"""Compile each spelling from another library's public type graph as a downstream import."""

import argparse
import json
from pathlib import Path
import tempfile

from probe_exports import bindings, compile_probe, invoke, pinned_environment, sha, spelling


def verify_configs(pin, entry, env):
    actual = {}

    def collect(role, directory):
        for name in ("config", "config.toml"):
            path = directory / name
            if path.exists():
                actual[f"{role}/{name}"] = sha(path.read_bytes())

    for role, directory in (("project", entry.parent), ("driver", Path.cwd().resolve())):
        for depth, ancestor in enumerate((directory, *directory.parents)):
            collect(f"{role}-{depth}", ancestor / ".cargo")
    home = env.get("CARGO_HOME") or (Path(env["HOME"]) / ".cargo" if "HOME" in env else None)
    if home is not None:
        collect("cargo-home", Path(home))
    if actual != pin["cargoConfigSha256"]:
        raise ValueError("Cargo configuration differs from subject export lock")


def locked_graph(path):
    raw = path.read_bytes()
    graph = json.loads(raw)
    lock_bytes = path.with_name("exports.lock").read_bytes()
    lock = json.loads(lock_bytes)
    if (
        lock["exportsSha256"] != sha(raw)
        or graph["vocabulary"] != "oddlang-public-type-bindings-v1"
        or lock["vocabulary"] != graph["vocabulary"]
        or lock["pin"]["targetKind"] != "lib"
    ):
        raise ValueError("expected digest-pinned library type graph")
    return raw, graph, lock, sha(lock_bytes)


def probe(expected, subject, manifest, cache):
    raw, graph, lock, subject_lock_hash = locked_graph(subject / "exports.json")
    reference, reference_graph, reference_lock, expected_lock_hash = locked_graph(expected)
    pin = lock["pin"]
    if pin["packageManifestSha256"] != sha(manifest.read_bytes()):
        raise ValueError("package manifest differs from subject export lock")
    if any(pin[key] != reference_lock["pin"][key] for key in ("compilerCommit", "targetTriple")):
        raise ValueError("reference and subject need the same compiler and target")
    env = pinned_environment()
    entry = next(
        (
            directory / "Cargo.toml"
            for directory in manifest.parents
            if (directory / "Cargo.toml").is_file()
            and sha((directory / "Cargo.toml").read_bytes()) == pin["manifestSha256"]
        ),
        None,
    )
    if entry is None:
        raise ValueError("workspace entry manifest differs from subject export lock")
    verify_configs(pin, entry, env)
    version = invoke(["rustc", "+1.97.1", "-vV"], env=env).stdout
    if (
        f"commit-hash: {pin['compilerCommit']}\n" not in version
        or f"host: {pin['targetTriple']}\n" not in version
    ):
        raise ValueError("native compiler profile differs from subject lock")
    opts = ["--manifest-path", manifest, "--locked", "--offline"]
    metadata = json.loads(
        invoke(
            ["cargo", "+1.97.1", "metadata", "--no-deps", "--format-version", "1", *opts],
            env=env,
            cwd=entry.parent,
        ).stdout
    )
    workspace = Path(metadata["workspace_root"])
    package = next(p for p in metadata["packages"] if Path(p["manifest_path"]) == manifest)
    if (
        package["name"] != pin["package"]
        or sha((workspace / "Cargo.lock").read_bytes()) != pin["cargoLockSha256"]
    ):
        raise ValueError("package selection or Cargo.lock differs from subject lock")
    if pin["sourceRevision"] is not None:
        revision = invoke(["git", "-C", workspace, "rev-parse", "HEAD"], env=env).stdout.strip()
        clean = invoke(
            ["git", "-C", workspace, "status", "--porcelain", "--untracked-files=all"], env=env
        ).stdout
        if revision != pin["sourceRevision"] or clean:
            raise ValueError("source revision differs from subject export lock")
    opts.extend(["--target", pin["targetTriple"], "--lib", "--package", pin["package"], "--release"])
    if not pin["defaultFeatures"]:
        opts.append("--no-default-features")
    for feature in pin["features"]:
        opts.extend(["--features", feature])
    output = invoke(
        ["cargo", "+1.97.1", "check", *opts, "--target-dir", cache, "--message-format=json"],
        env=env,
        cwd=entry.parent,
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
    edges = bindings(reference_graph, "bindings")
    known = bindings(graph, "bindings")
    expected_ambiguous = bindings(reference_graph, "ambiguous")
    subject_ambiguous = bindings(graph, "ambiguous")
    unresolved = {
        tuple(row["path"]) + (name,) for row in graph["modules"].values() for name in row["unresolved"]
    }
    observations = []
    with tempfile.TemporaryDirectory() as directory:
        temp = Path(directory)
        for path in sorted(edges.keys() | expected_ambiguous.keys()):
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
                    "expectedKind": edges[path]["kind"] if path in edges else "ambiguous",
                    "subjectKind": known[path]["kind"] if path in known else None,
                    "importCompiles": result.returncode == 0,
                    "errorCodes": codes,
                }
            )
            if path in expected_ambiguous:
                observations[-1]["expectedAmbiguity"] = expected_ambiguous[path]
            if path in subject_ambiguous:
                observations[-1]["subjectAmbiguity"] = subject_ambiguous[path]
            if path in unresolved:
                observations[-1]["subjectUnresolved"] = True
        # A subject with no shared spellings must still resolve under its external alias.
        compile_probe(["pub use observed as subject;"], temp, files[0], env)
    return {
        "expectedExportsSha256": sha(reference),
        "expectedLockSha256": expected_lock_hash,
        "expectedUnresolved": sorted(
            list(row["path"]) + [name]
            for row in reference_graph["modules"].values()
            for name in row["unresolved"]
        ),
        "subjectLockSha256": subject_lock_hash,
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
