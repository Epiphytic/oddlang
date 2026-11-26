#!/usr/bin/env python3
"""Replay frozen stream obligations against a native caller or a linearly invoked peer."""
import argparse
import base64
import copy
import gzip
import importlib.util
import json
import os
from pathlib import Path
import platform
import shlex
import subprocess
import sys
import tempfile
import zlib

from seeded_cases import cases as seeded_cases
from spec_cases import b64, canonical, cases as spec_cases, sha


ROOT = Path(__file__).resolve().parent
PRIOR = ROOT.parent / "stream-driver"
VECTORS = ROOT.parent / "stream-vectors.json"
FREEZE = ROOT / "freeze.json"
ARCHIVED_SPEC = importlib.util.spec_from_file_location("archived_stream_oracle", PRIOR / "run.py")
ARCHIVED = importlib.util.module_from_spec(ARCHIVED_SPEC)
ARCHIVED_SPEC.loader.exec_module(ARCHIVED)
expand, matches = ARCHIVED.expand, ARCHIVED.matches

CARGO = ["cargo", "+1.97.1"]
RUSTC = ["rustc", "+1.97.1"]
TARGET = "aarch64-apple-darwin"
UPSTREAM = "49fc705408a230bd0ad896248d08dbdd2faad05a"
CARGO_LOCK = "9c5f7dde5c0f7d340e22dbefaa8a1c6ddb94c119011ba3e49ac8044c9b4cb0b3"


def require(condition, problem):
    if not condition:
        raise ValueError(problem)


def digest(path):
    return sha(path.read_bytes()).hex()


def write(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def corpus_digest(cases):
    return sha(json.dumps(cases, sort_keys=True, separators=(",", ":")).encode()).hex()


def validate_pin(workspace):
    revision = subprocess.check_output(
        ["git", "-C", workspace, "rev-parse", "HEAD"], text=True, timeout=20,
    ).strip()
    require(revision == UPSTREAM, f"workspace revision differs: {revision}")
    status = subprocess.check_output(
        ["git", "-C", workspace, "status", "--porcelain", "--untracked-files=all"],
        text=True, timeout=20,
    )
    require(not status, f"workspace contains modifications: {status}")
    lock = digest(workspace / "Cargo.lock")
    require(lock == CARGO_LOCK, f"unexpected Cargo.lock digest: {lock}")
    return {"revision": revision, "cargoLockSha256": lock}


def build_original(workspace, target):
    provenance = validate_pin(workspace)
    compiler = subprocess.check_output([*RUSTC, "-vV"], text=True, timeout=20)
    require(f"host: {TARGET}\n" in compiler, f"requires observed target {TARGET}")
    require(compiler.startswith("rustc 1.97.1 "), "expected Rust 1.97.1")
    cargo_command = [
        *CARGO, "build", "--locked", "--offline", "--target", TARGET,
        "--package", "marmish-stream", "--target-dir", str(target),
        "--message-format=json-render-diagnostics",
    ]
    cargo_environment = {key: value for key, value in os.environ.items()
                         if key in {"HOME", "PATH", "CARGO_HOME", "RUSTUP_HOME"}}
    cargo_environment.update(CARGO_NET_OFFLINE="true", LANG="C", LC_ALL="C")
    result = subprocess.run(cargo_command, cwd=workspace, env=cargo_environment,
                            check=True, stdout=subprocess.PIPE, timeout=180)
    libraries = {}
    features = {}
    for raw in result.stdout.splitlines():
        message = json.loads(raw)
        if message["reason"] != "compiler-artifact":
            continue
        name = message["target"]["name"]
        if name in {"marmish_stream", "serde", "serde_json", "flate2"}:
            require(name not in features, f"ambiguous compiler artifact for {name}")
            features[name] = message["features"]
        if name in {"marmish_stream", "serde", "serde_json"}:
            (path,) = (Path(filename) for filename in message["filenames"] if filename.endswith(".rlib"))
            require(path.resolve().is_relative_to(target.resolve()) and not message["fresh"],
                    f"expected a fresh linked library under the chosen target: {path}")
            libraries[name] = path
    require(set(libraries) == {"marmish_stream", "serde", "serde_json"}, "missing linked libraries")
    caller = target / "stream-oracle"
    sources = {"stream.rs": digest(ROOT / "stream.rs"), "archived-main.rs": digest(PRIOR / "main.rs")}
    linked = {name: digest(path) for name, path in libraries.items()}
    binding = corpus_digest({"upstream": provenance, "compiler": compiler, "sources": sources, "libs": linked})
    rustc_command = [
        *RUSTC, "--edition", "2024", "--crate-name", "stream_oracle",
        "--target", TARGET, str(ROOT / "stream.rs"), "-L",
        f"dependency={target / TARGET / 'debug' / 'deps'}",
        "-L", f"dependency={target / 'debug' / 'deps'}", "-o", str(caller),
    ]
    for name, path in sorted(libraries.items()):
        rustc_command.extend(["--extern", f"{name}={path}"])
    cargo_environment.update(ODDLANG_STREAM_DRIVER_BINDING=binding, ODDLANG_STREAM_DRIVER_ROLE="original")
    subprocess.run(rustc_command, env=cargo_environment, cwd=workspace, check=True, timeout=45)
    require(validate_pin(workspace) == provenance, "upstream identity changed during build")
    require({name: digest(path) for name, path in libraries.items()} == linked,
            "linked libraries changed during build")
    provenance.update({
        "features": features, "profile": "dev", "target": TARGET, "compiler": compiler.splitlines(),
        "callerBinding": binding, "callerSha256": digest(caller), "librarySha256": linked,
        "sourceSha256": sources, "cargoCommand": shlex.join(cargo_command),
        "rustcCommand": shlex.join(rustc_command), "buildLocale": "C",
    })
    return [str(caller)], provenance


def observe(command, cases, binding=None):
    inputs = [{key: row[key] for key in ("name", "operation", "input")} for row in cases]
    with tempfile.TemporaryDirectory(prefix="oddlang-stream-peer-") as cwd:
        result = subprocess.run(
            command, input=canonical(inputs).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            cwd=cwd, env={"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C", "TZ": "UTC"},
            timeout=30, check=False,
        )
    require(result.returncode == 0 and not result.stderr,
            f"adapter status {result.returncode}, stderr {result.stderr.decode(errors='replace')}")
    envelope = json.loads(result.stdout)
    if binding is not None:
        require(envelope["binding"] == binding, "adapter artifact binding mismatch")
    rows = envelope["cases"]
    require([row["name"] for row in rows] == [row["name"] for row in cases],
            "adapter returned missing, duplicate or reordered observations")
    return rows


def failures(cases, observations):
    require([row["name"] for row in observations] == [row["name"] for row in cases],
            "observation identifiers differ")
    mismatches = []
    for case, report in zip(cases, observations):
        actual, expected = report["actual"], expand(case["expected"])
        if isinstance(expected, dict) and "zlib_roundtrip_hex" in expected:
            valid = isinstance(actual, dict) and set(actual) == {"codec", "offset", "raw_len", "data", "sha256"}
        else:
            valid = True
        if not valid or not matches(actual, expected):
            mismatches.append(case["name"])
    return mismatches


def negative_controls(cases, observations):
    scenarios = [
        ("copy-data-tag", "binary-data-encode", lambda tag: "0c" + tag[2:]),
        ("error-precedence", "codec-block-length-before-offset", lambda _: {"error": "Offset"}),
        ("rejected-transition", "seed-0-receive-retry", lambda steps: [
            {**step, "delivered": step["delivered"] + 1} if index == 7 else step
            for index, step in enumerate(steps)
        ]),
    ]
    result = {}
    for mutation, name, change in scenarios:
        mutated = copy.deepcopy(cases)
        case = next(row for row in mutated if row["name"] == name)
        case["expected"] = change(case["expected"])
        result[mutation] = failures(mutated, observations)
        require(result[mutation] == [name], f"undetected mutation or unrelated failure: {mutation}")
    return result


def replay_peer(writer, reader, primary_cases, writer_binding=None, reader_binding=None):
    indexed = {row["name"]: row for row in primary_cases}
    pairs = [
        (name, name.removesuffix("-encode") + "-decode")
        for name, row in indexed.items()
        if row["operation"] in {"frame_encode", "request_encode", "response_encode"}
        and name.endswith("-encode") and name.removesuffix("-encode") + "-decode" in indexed
    ]
    written = observe(writer, [indexed[name] for name, _ in pairs], writer_binding)
    writes = [indexed[name] for name, _ in pairs]
    require(not failures(writes, written), "mixed peer write disagreed with exact wire expectation")
    reads = [indexed[name] | {"input": observation["actual"]}
             for (_, name), observation in zip(pairs, written)]
    received = observe(reader, reads, reader_binding)
    require(not failures(reads, received), "mixed peer reader rejected conforming peer bytes")
    return [record["name"] for record in received]


def zlib_peers(command, cases, observations, binding):
    index = {row["name"]: row for row in cases}
    shape = next(row["actual"] for row in observations if row["name"] == "codec-zlib-encode")
    expected = index["codec-zlib-encode"]["expected"]
    raw = bytes.fromhex(expand(expected["zlib_roundtrip_hex"]))
    written = base64.b64decode(shape["data"], validate=True)
    alternatives = [zlib.compress(raw, level=level) for level in (1, 9)]
    alternate = next((data for data in alternatives if data != written and len(data) < len(raw)), None)
    require(alternate is not None, "alternate zlib fixture did not differ")
    read = {
        "name": "independent-zlib-write", "operation": "codec_decode",
        "input": {"block": shape | {"data": b64(alternate)}, "negotiated": "zlib"},
        "expected": raw.hex(),
    }
    received = observe(command, [read], binding)
    require(not failures([read], received), "Rust rejected independently compressed payload")
    require(matches(shape, expand(expected)), "independent zlib decoder rejected Rust output")
    return {
        "rawSha256": sha(raw).hex(), "rawBytes": len(raw),
        "rustZlibHex": written.hex(), "independentZlibHex": alternate.hex(),
        "pythonZlibVersion": zlib.ZLIB_RUNTIME_VERSION,
        "directions": ["Rust encoder to Python decoder", "Python encoder to Rust decoder"],
    }


def sealed_cases(path, freeze):
    data = json.loads(path.read_bytes())
    require(digest(path) == freeze["holdout"]["fileSha256"], "holdout file does not match commitment")
    derived = seeded_cases(data["seed"], data["rounds"])[:-1]
    for row in derived:
        row["name"] = row["name"].replace("seed-", "holdout-", 1)
    require(len(derived) == freeze["holdout"]["cases"], "incorrect holdout count")
    require(data["cases"] == derived and corpus_digest(derived) == freeze["holdout"]["corpusSha256"],
            "holdout expectations do not match sealed seed and generator")
    return derived


def holdout_boundary(path):
    # The sealed file is read by the judge before launching the isolated caller.
    sandbox = Path("/usr/bin/sandbox-exec")
    require(sandbox.is_file(), "holdout replay requires the macOS qualification boundary")
    directory = json.dumps(str(path.resolve().parent))
    boundary = [
        str(sandbox), "-p",
        f"(version 1)(allow default)(deny file-read* file-write* (subpath {directory}))",
    ]
    probe = subprocess.run(
        [*boundary, "/bin/cat", str(path.resolve())],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10,
    )
    require(probe.returncode == 1 and not probe.stdout and b"Operation not permitted" in probe.stderr,
            "qualification boundary did not deny a direct holdout read")
    return boundary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, help="clean native checkout at the pin")
    parser.add_argument("--command", nargs="+", help="absolute adapter program and arguments")
    parser.add_argument("--peer-command", nargs="+", help="optional separately built adapter")
    parser.add_argument("--holdout", type=Path, help="qualification-owned sealed input and expectations")
    parser.add_argument("--observations", type=Path, help="optional gzip of public observations")
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    require(bool(args.upstream) != bool(args.command), "select exactly one of --upstream or --command")
    freeze = json.loads(FREEZE.read_bytes())
    require(freeze["schema"] == 1 and freeze["revision"] == UPSTREAM, "invalid freeze header")
    prior = json.loads(VECTORS.read_bytes())
    require(prior["schema"] == 1 and prior["upstream_commit"] == UPSTREAM, "prior corpus header mismatch")
    require(digest(VECTORS) == freeze["priorCorpusSha256"], "prior corpus digest mismatch")
    spec = spec_cases()
    seeded = seeded_cases(freeze["seed"], freeze["seededRounds"])
    require(corpus_digest(spec) == freeze["authoredCorpusSha256"], "authored corpus digest mismatch")
    require(corpus_digest(seeded) == freeze["seededCorpusSha256"], "seeded corpus digest mismatch")
    cases = prior["cases"] + spec + seeded
    require(len({row["name"] for row in cases}) == len(cases), "duplicate public corpus ID")
    # Holdouts use independent inputs with distinct names. Do not emit their raw observations here.
    holdout = sealed_cases(args.holdout, freeze) if args.holdout else None
    with tempfile.TemporaryDirectory(prefix="oddlang-portable-stream-build-") as directory:
        if args.upstream:
            command, provenance = build_original(args.upstream.resolve(), Path(directory))
        else:
            command, provenance = args.command, {"callerSha256": digest(Path(args.command[0]))}
        binding = provenance.get("callerBinding")
        observations = observe(command, cases, binding)
        mismatches = failures(cases, observations)
        if mismatches:
            first = next(row for row in observations if row["name"] == mismatches[0])
            raise ValueError(f"failed vectors {mismatches}; first observation {first!r}")
        controls = negative_controls(cases, observations)
        compressed = zlib_peers(command, cases, observations, binding)
        require(digest(Path(command[0])) == provenance["callerSha256"], "adapter changed during replay")
        report = {
            "schema": 1, "provenance": provenance, "freezeSha256": digest(FREEZE),
            "oracleSourceSha256": {
                name: digest(path) for name, path in [
                    ("run.py", Path(__file__)),
                    ("spec_cases.py", ROOT / "spec_cases.py"),
                    ("seeded_cases.py", ROOT / "seeded_cases.py"),
                    ("archived-run.py", PRIOR / "run.py"),
                ]
            },
            "runtime": {"platform": platform.platform(), "python": platform.python_version(),
                        "locale": "C", "timezone": "UTC", "environment": {"PATH": "/usr/bin:/bin"}},
            "priorCases": len(prior["cases"]), "authoredCases": len(spec), "seededCases": len(seeded),
            "publicObservationSha256": corpus_digest(observations),
            "negativeControls": controls, "alternateZlib": compressed,
            "mixedBinaryAndJson": {"writerToReader": replay_peer(command, args.peer_command or command,
                                                                 cases, binding, None)},
            "holdout": None,
        }
        if args.peer_command:
            report["mixedBinaryAndJson"]["readerToWriter"] = replay_peer(
                args.peer_command, command, cases, None, binding,
            )
        if holdout is not None:
            boundary = holdout_boundary(args.holdout)
            try:
                result = observe([*boundary, *command], holdout, binding)
                require(not failures(holdout, result), "sealed holdout comparison failed")
                if args.peer_command:
                    peer_result = observe([*boundary, *args.peer_command], holdout)
                    require(not failures(holdout, peer_result), "sealed holdout peer comparison failed")
            except (OSError, ValueError, subprocess.SubprocessError):
                # A faulty adapter may write case contents to stderr.
                raise ValueError("sealed holdout replay failed") from None
            report["holdout"] = {
                "cases": len(holdout), "fileSha256": digest(args.holdout), "passed": True,
                "directReadDenied": True, "peerCompared": bool(args.peer_command),
            }
    if args.observations:
        args.observations.write_bytes(gzip.compress((canonical(observations) + "\n").encode(), mtime=0))
        report["publicObservationArchiveSha256"] = digest(args.observations)
    write(args.receipt, report)
    print(f"{len(cases)} public stream cases; {len(holdout or [])} sealed cases; "
          f"{len(controls)} detected mutations; {len(report['mixedBinaryAndJson']['writerToReader'])} wire replays")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as failure:
        sys.exit(str(failure))
