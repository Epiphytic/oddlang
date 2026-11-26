#!/usr/bin/env python3
"""Observe the independent stream vectors through one freshly built workspace."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import zlib

DRIVER = Path(__file__).resolve().parent
TOOLCHAIN = "+1.97.1"
STREAM = Path("crates/marmish-stream")


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def binding_digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def identity(args, workspace, upstream):
    baseline = json.loads(args.baseline.read_text())
    if baseline["upstreamCommit"] != upstream:
        raise ValueError("workspace baseline pins an unexpected upstream")
    lock_inputs = baseline["cargoInputs"]
    actual_inputs = {relative: digest(workspace / relative) for relative in lock_inputs}
    if actual_inputs != lock_inputs:
        raise ValueError("driver workspace manifests or lock differ from the freeze")
    actual_paths = sorted(path.relative_to(workspace).as_posix()
                          for path in (workspace / STREAM).rglob("*.rs"))
    if actual_paths != sorted(baseline["trackedSources"]):
        raise ValueError("driver workspace stream Rust source set differs from the freeze")
    if any((workspace / path).is_symlink() for path in actual_paths):
        raise ValueError("driver workspace source links to an unverified file")
    actual_sources = {path: digest(workspace / path) for path in actual_paths}
    expected = {path: record["sha256"] for path, record in baseline["trackedSources"].items()}
    if (actual_sources != expected) != (args.role == "mutation"):
        raise ValueError("driver source bytes disagree with the requested build role")

    provenance = {"role": args.role, "root": str(workspace),
                  "baselineSha256": digest(args.baseline),
                  "cargoInputs": actual_inputs, "sources": actual_sources}
    if args.archive_sha256:
        if not re.fullmatch(r"[0-9a-f]{64}", args.archive_sha256) or not re.fullmatch(
                r"[0-9a-f]{40}", args.commit or ""):
            raise ValueError("archive provenance needs a full commit and SHA-256")
        provenance.update(commit=args.commit, archiveSha256=args.archive_sha256)
    elif args.role == "original":
        commit = subprocess.check_output(["git", "-C", workspace, "rev-parse", "HEAD"],
                                         text=True).strip()
        provenance["commit"] = commit
    if args.role == "original" and provenance.get("commit") != upstream:
        raise ValueError("original driver must execute from the pinned upstream")
    if args.role == "candidate":
        if not args.archive_sha256 or args.commit == upstream or not args.bundle_sha256:
            raise ValueError("candidate driver requires qualified downstream archive and bundle pins")
        actual_bundle = digest(workspace / "oddlang/stream/bundle.lock")
        if actual_bundle != args.bundle_sha256:
            raise ValueError("candidate driver bundle pin disagrees with the staged checkout")
        provenance["bundleSha256"] = actual_bundle
    return provenance


def expand(value):
    if isinstance(value, list):
        return [expand(entry) for entry in value]
    if isinstance(value, dict):
        if set(value) == {"prefix", "byte", "repeat"}:
            return value["prefix"] + f"{value['byte']:02x}" * value["repeat"]
        return {key: expand(entry) for key, entry in value.items()}
    return value


def tool(name, toolchain):
    return [str(toolchain / "bin" / name)] if toolchain else [name, TOOLCHAIN]


def rust_libraries(manifest, target, cargo, boundary):
    command = [
        *cargo, "build", "--locked", "--offline", "-p", "marmish-stream",
        "--manifest-path", str(manifest), "--target-dir", str(target),
        "--message-format=json-render-diagnostics",
    ]
    build = subprocess.run([*boundary, *command], cwd=manifest.parent, check=True,
                           stdout=subprocess.PIPE)
    libs = {}
    for line in build.stdout.splitlines():
        message = json.loads(line)
        if message["reason"] != "compiler-artifact":
            continue
        name = message["target"]["name"]
        if name not in {"marmish_stream", "serde_json"}:
            continue
        (rlib,) = (Path(value) for value in message["filenames"] if value.endswith(".rlib"))
        rlib.resolve().relative_to(target)
        if message["fresh"] or name in libs:
            raise ValueError(f"unexpected cached or ambiguous artifact for {name}")
        libs[name] = rlib
    if set(libs) != {"marmish_stream", "serde_json"}:
        raise ValueError("build did not report both downstream driver libraries")
    root = target / "debug" / "deps"
    if any(rlib.parent.resolve() not in {root, root.parent} for rlib in libs.values()):
        raise ValueError("Cargo artifact outside the selected native target")
    return libs, command


def compile_driver(target, libs, role, attachment, rustc, source, boundary):
    executable = target / "stream-oracle"
    command = [
        *rustc, "--edition=2024", "--crate-name", "stream_oracle",
        str(source), "-L", f"dependency={target / 'debug' / 'deps'}",
        "-o", str(executable),
    ]
    for name, path in sorted(libs.items()):
        command += ["--extern", f"{name}={path}"]
    env = os.environ | {
        "ODDLANG_STREAM_DRIVER_ROLE": role,
        "ODDLANG_STREAM_DRIVER_BINDING": attachment,
    }
    subprocess.run([*boundary, *command], env=env, check=True)
    return executable, command


def zlib_matches(actual, expected):
    stream = zlib.decompressobj()
    encoded = base64.b64decode(actual["data"], validate=True)
    raw = stream.decompress(encoded, 8193)
    return all((
        actual["codec"] == "zlib",
        actual["offset"] == expected["offset"],
        actual["raw_len"] == len(raw),
        actual["sha256"] == expected["sha256"],
        len(encoded) < len(raw),
        raw.hex() == expected["zlib_roundtrip_hex"],
        stream.eof,
        not stream.unused_data,
        not stream.unconsumed_tail,
    ))


def matches(actual, expected):
    if isinstance(expected, dict) and "zlib_roundtrip_hex" in expected:
        try:
            return zlib_matches(actual, expected)
        except (KeyError, TypeError, ValueError, zlib.error):
            return False
    if isinstance(expected, dict):
        return (
            isinstance(actual, dict)
            and actual.keys() == expected.keys()
            and all(matches(actual[key], value) for key, value in expected.items())
        )
    if isinstance(expected, list):
        return (
            isinstance(actual, list)
            and len(actual) == len(expected)
            and all(matches(value, other) for value, other in zip(actual, expected))
        )
    return type(actual) is type(expected) and actual == expected


def run_vectors(executable, role, attachment, cases, boundary=(), cwd=None):
    executable_digest = digest(executable)
    run = subprocess.run(
        [*boundary, str(executable)], cwd=cwd, input=json.dumps(cases),
        text=True, check=True, stdout=subprocess.PIPE,
    )
    result = json.loads(run.stdout)
    if result["role"] != role:
        raise ValueError(f"driver contains role {result['role']!r}, expected {role!r}")
    if result["binding"] != attachment:
        raise ValueError("driver executable is not bound to the selected workspace and artifacts")
    if digest(executable) != executable_digest:
        raise ValueError("driver executable changed during execution")
    if [case["name"] for case in result["cases"]] != [case["name"] for case in cases]:
        raise ValueError("missing or unexpected driver cases")
    observations = []
    for case, observed in zip(cases, result["cases"]):
        expected = expand(case["expected"])
        observations.append(observed | {"matched": matches(observed["actual"], expected)})
    return executable_digest, observations


def peer_vectors(peer, cases, vectors_digest, driver_digest, identity):
    role = identity["role"]
    other = {"original": "candidate", "candidate": "original"}.get(role)
    if other is None or peer["role"] != other:
        raise ValueError("wire replay requires distinct original and candidate roles")
    if (peer["vectors_sha256"] != vectors_digest or peer["driver_source_sha256"] != driver_digest
            or not peer["all_matched"]):
        raise ValueError("wire replay requires a passing peer using the same frozen driver and vectors")
    provenance = peer["workspace_identity"]
    attachment = {"workspace": provenance, "libraries": peer["libraries"]}
    if (
        provenance["role"] != other or provenance["root"] == identity["root"]
        or any(provenance[key] != identity[key] for key in ("cargoInputs", "sources", "baselineSha256"))
        or binding_digest(attachment) != peer["driver_binding_sha256"]
    ):
        raise ValueError("peer receipt is not bound to a distinct build of the same frozen inputs")
    observations = peer["observations"]
    if [row["name"] for row in observations] != [case["name"] for case in cases]:
        raise ValueError("wrong peer observation membership")
    if not all(matches(row["actual"], expand(case["expected"]))
               for row, case in zip(observations, cases)):
        raise ValueError("peer receipt observations do not match the frozen vectors")
    actual = {record["name"]: record["actual"] for record in observations}
    indexed = {case["name"]: case for case in cases}
    pairs = [
        ("binary-data-encode", "binary-data-decode"),
        ("maximum-data-encode", "maximum-data-decode"),
        ("checkpoint-encode", "checkpoint-decode"),
        ("progress-encode", "progress-decode"),
        ("error-record-encode", "error-record-decode"),
        ("hello-request-encode", "hello-request-decode"),
        ("gap-response-encode", "gap-response-decode"),
    ]
    replay = [
        dict(indexed[decoder], name=f"{other}:{encoder}", input=actual[encoder])
        for encoder, decoder in pairs
    ]
    for name, negotiation in [("codec-uncompressed-encode", "zlib"), ("codec-zlib-encode", "zlib")]:
        case = indexed[name]
        replay.append({
            "name": f"{other}:{name}",
            "operation": "codec_decode",
            "input": {"block": actual[name], "negotiated": negotiation},
            "expected": case["input"]["bytes"],
        })
    return replay


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--role", choices=["original", "candidate", "mutation"], required=True)
    parser.add_argument("--peer-receipt", type=Path)
    parser.add_argument("--sandbox", type=Path)
    parser.add_argument("--toolchain", type=Path, help="explicit Rust 1.97.1 toolchain directory")
    parser.add_argument("--baseline", type=Path, required=True, help="frozen upstream build receipt")
    parser.add_argument("--commit", help="commit of the archived workspace")
    parser.add_argument("--archive-sha256", help="SHA-256 of the vetted Git archive")
    parser.add_argument("--bundle-sha256", help="verified candidate bundle digest")
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    target = args.target_dir.resolve()
    boundary = ["/usr/bin/sandbox-exec", "-f", str(args.sandbox.resolve())] if args.sandbox else []
    rustc = tool("rustc", args.toolchain)
    cargo = tool("cargo", args.toolchain)
    version = subprocess.check_output([*boundary, *rustc, "-vV"], text=True).strip()
    if not version.startswith("rustc 1.97.1 "):
        parser.error("expected the pinned Rust 1.97.1 compiler")
    if target.exists() and any(target.iterdir()):
        parser.error("target directory must be empty to rule out previously built artifacts")
    vectors = DRIVER.parent / "stream-vectors.json"
    driver_digest = digest(DRIVER / "main.rs")
    vectors_digest = digest(vectors)
    corpus = json.loads(vectors.read_text())
    cases = corpus["cases"]
    if corpus["schema"] != 1 or len(set(case["name"] for case in cases)) != len(cases):
        parser.error("invalid frozen vectors")
    try:
        provenance = identity(args, workspace, corpus["upstream_commit"])
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.error(str(error))

    libs, cargo_command = rust_libraries(workspace / "Cargo.toml", target, cargo, boundary)
    library_identities = {name: {"path": str(path), "sha256": digest(path)}
                          for name, path in libs.items()}
    attachment = binding_digest({"workspace": provenance, "libraries": library_identities})
    executable, rustc_command = compile_driver(
        target, libs, args.role, attachment, rustc, DRIVER / "main.rs", boundary,
    )
    if (identity(args, workspace, corpus["upstream_commit"]) != provenance
            or any(digest(path) != library_identities[name]["sha256"] for name, path in libs.items())):
        raise ValueError("workspace or linked library changed before driver execution")
    binary_digest, observed = run_vectors(executable, args.role, attachment, cases, boundary, workspace)
    receipt = {
        "schema": 1,
        "role": args.role,
        "workspace": str(workspace),
        "target": str(target),
        "upstream_commit": corpus["upstream_commit"],
        "lock_sha256": digest(workspace / "Cargo.lock"),
        "vectors_sha256": vectors_digest,
        "driver_source_sha256": driver_digest,
        "driver_sha256": binary_digest,
        "driver_path": str(executable),
        "driver_binding_sha256": attachment,
        "workspace_identity": provenance,
        "cargo_command": shlex.join(cargo_command),
        "rustc_command": shlex.join(rustc_command),
        "rustc_version": version,
        "libraries": library_identities,
        "observations": observed,
    }
    if args.peer_receipt:
        peer = json.loads(args.peer_receipt.read_text())
        if peer["driver_sha256"] == binary_digest:
            raise ValueError("peer and local driver binaries have identical identities")
        replay = peer_vectors(peer, cases, vectors_digest, driver_digest, provenance)
        replay_digest, mixed = run_vectors(executable, args.role, attachment, replay, boundary, workspace)
        if replay_digest != binary_digest:
            raise ValueError("mixed wire replay used a different executable")
        receipt["peer_receipt_sha256"] = digest(args.peer_receipt)
        receipt["mixed_observations"] = mixed

    failures = [case["name"] for case in observed + receipt.get("mixed_observations", [])
                if not case["matched"]]
    receipt["all_matched"] = not failures
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2) + "\n")
    if failures:
        parser.exit(1, "Failed vectors: " + ", ".join(failures) + "\n")
    print(f"{args.role}: {len(observed)} vectors, "
          f"{len(receipt.get('mixed_observations', []))} mixed observations, "
          f"binary {binary_digest}")


if __name__ == "__main__":
    main()
