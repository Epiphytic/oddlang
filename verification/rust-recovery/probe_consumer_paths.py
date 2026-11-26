"""Compile each spelling from another library's public type graph as a downstream import."""

import argparse
import json
from pathlib import Path
import tempfile

from probe_exports import bindings, compile_probe, invoke, pinned_environment, sha, spelling


def probe(expected, subject, manifest, cache):
    raw = (subject / "exports.json").read_bytes()
    lock = json.loads((subject / "exports.lock").read_bytes())
    pin = lock["pin"]
    if lock["exportsSha256"] != sha(raw) or pin["packageManifestSha256"] != sha(manifest.read_bytes()):
        raise ValueError("subject graph or package manifest differs from the export lock")
    env = pinned_environment()
    version = invoke(["rustc", "+1.97.1", "-vV"], env=env).stdout
    if (
        f"commit-hash: {pin['compilerCommit']}\n" not in version
        or f"host: {pin['targetTriple']}\n" not in version
    ):
        raise ValueError("native compiler profile differs from subject lock")
    opts = ["--manifest-path", manifest, "--locked", "--offline"]
    metadata = json.loads(
        invoke(
            ["cargo", "+1.97.1", "metadata", "--no-deps", "--format-version", "1", *opts], env=env
        ).stdout
    )
    workspace = Path(metadata["workspace_root"])
    package = next(p for p in metadata["packages"] if Path(p["manifest_path"]) == manifest)
    if (
        package["name"] != pin["package"]
        or sha((workspace / "Cargo.lock").read_bytes()) != pin["cargoLockSha256"]
    ):
        raise ValueError("package selection or Cargo.lock differs from subject lock")
    opts.extend(["--target", pin["targetTriple"], "--lib", "--package", pin["package"], "--release"])
    if not pin["defaultFeatures"]:
        opts.append("--no-default-features")
    for feature in pin["features"]:
        opts.extend(["--features", feature])
    output = invoke(
        ["cargo", "+1.97.1", "check", *opts, "--target-dir", cache, "--message-format=json"], env=env
    )
    rows = [json.loads(line) for line in output.stdout.splitlines()]
    files = [
        Path(f)
        for row in rows
        if row.get("reason") == "compiler-artifact"
        and row["package_id"] == package["id"]
        and row["target"]["name"] == pin["targetName"]
        and "lib" in row["target"]["kind"]
        for f in row["filenames"]
        if f.endswith(".rmeta")
    ]
    if len(files) != 1:
        raise ValueError("expected one selected library metadata artifact")
    reference = expected.read_bytes()
    edges = bindings(json.loads(reference), "bindings")
    known = bindings(json.loads(raw), "bindings")
    observations = []
    with tempfile.TemporaryDirectory() as directory:
        temp = Path(directory)
        for path in sorted(edges):
            result = compile_probe(
                [f"pub use {spelling(('observed',) + path)} as witness;"],
                temp,
                files[0],
                env,
                lint="#![deny(ambiguous_glob_imports)]",
                expect=False,
            )
            diagnostics = [
                json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")
            ]
            codes = sorted(
                {
                    row["code"]["code"]
                    for row in diagnostics
                    if row.get("level") == "error" and row.get("code")
                }
            )
            if result.returncode and (
                not codes or not set(codes) <= {"E0432", "E0433", "E0603", "ambiguous_glob_imports"}
            ):
                raise ValueError(f"unexpected downstream diagnostic for {path}: {result.stderr}")
            if path in known and result.returncode:
                raise ValueError(f"subject's own public binding failed to compile: {path}")
            observations.append(
                {
                    "spelling": list(path),
                    "expectedKind": edges[path]["kind"],
                    "subjectKind": known[path]["kind"] if path in known else None,
                    "importCompiles": result.returncode == 0,
                    "errorCodes": codes,
                }
            )
        # A subject with no shared spellings must still resolve under its external alias.
        compile_probe(["pub use observed as subject;"], temp, files[0], env)
    return {
        "expectedExportsSha256": sha(reference),
        "subjectLockSha256": sha((subject / "exports.lock").read_bytes()),
        "subjectPackage": pin["package"],
        "subjectTargetName": pin["targetName"],
        "subjectExportsSha256": sha(raw),
        "compiler": pin["compilerCommit"],
        "target": pin["targetTriple"],
        "imports": observations,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expected", type=Path, help="reference exports.json")
    parser.add_argument(
        "subject", type=Path, help="directory containing the subject exports.json and exports.lock"
    )
    parser.add_argument("manifest", type=Path, help="subject library Cargo.toml")
    parser.add_argument(
        "target_dir", type=Path, help="disposable or previously used Cargo target directory"
    )
    parser.add_argument("receipt", type=Path)
    args = parser.parse_args()
    result = probe(args.expected, args.subject, args.manifest.resolve(), args.target_dir.resolve())
    args.receipt.write_text(json.dumps(result, sort_keys=True, indent=2) + "\n")
