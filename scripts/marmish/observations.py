"""Run frozen framing observations against the pinned, offline Marmish core."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

HERE = Path(__file__).resolve().parent
CONTRACT = HERE.parents[1] / "examples/marmish/observations.json"
CONSUMER = HERE / "observe_core.rs"
REVISION = "49fc705408a230bd0ad896248d08dbdd2faad05a"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def expand(segments):
    data = bytearray()
    for item in segments:
        if set(item) == {"utf8"}:
            part = item["utf8"].encode()
        elif set(item) == {"hex"}:
            part = bytes.fromhex(item["hex"])
        elif set(item) == {"repeatHex", "count"} and type(item["count"]) is int and item["count"] >= 0:
            part = bytes.fromhex(item["repeatHex"]) * item["count"]
        else:
            raise ValueError(f"invalid byte segment: {item}")
        data.extend(part)
    return bytes(data)


def run(args, cwd, **kwargs):
    return subprocess.run(args, cwd=cwd, capture_output=True, check=True, **kwargs).stdout


def observe(binary, case):
    operation = case["operation"]
    if operation not in ("encode", "decode"):
        raise ValueError(f"unknown operation: {operation}")
    data = expand(case["input"])
    observed = subprocess.run([binary, operation], input=data, capture_output=True, check=False)
    if observed.returncode == 0 and observed.stderr == b"":
        actual = {"bytesHex": observed.stdout.hex()}
    elif observed.returncode == 2 and observed.stdout == b"" and observed.stderr.endswith(b"\n"):
        actual = {"error": observed.stderr[:-1].decode("ascii")}
    else:
        raise ValueError(f"{case['id']} consumer failure {observed.returncode}: {observed.stderr!r}")
    expected = case["expected"]
    if set(expected) == {"bytes"}:
        expected = {"bytesHex": expand(expected["bytes"]).hex()}
    if expected != actual:
        raise ValueError(f"{case['id']}: expected {str(expected)[:150]}, got {str(actual)[:150]}")
    return {"id": case["id"], "operation": operation, "inputSha256": digest(data), "observed": actual}


def build_core_consumer(upstream, temp, env):
    env["CARGO_TARGET_DIR"] = str(temp / "target")
    rustc = run(["rustc", "+1.97.1", "-Vv"], upstream, env=env).decode().strip()
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    build = run(["cargo", "+1.97.1", "build", "--locked", "--offline", "-p", "marmish-core",
                 "--lib", "--message-format=json", "--target", host], upstream, env=env)
    artifacts = [m for s in build.splitlines() if (m := json.loads(s))["reason"] == "compiler-artifact"
                 and m["target"]["name"] == "marmish_core"]
    [artifact] = artifacts
    [rlib] = [Path(p) for p in artifact["filenames"] if p.endswith(".rlib")]
    deps = temp / "target" / host / "debug/deps"
    host_deps = temp / "target/debug/deps"
    consumer = temp / "consumer"
    run(["rustc", "+1.97.1", "--edition=2024", "--target", host, "--extern",
         f"marmish_core={rlib}", "-L", f"dependency={deps}", "-L", f"dependency={host_deps}", CONSUMER, "-o", consumer], upstream, env=env)
    run([consumer, "api"], upstream, env=env)
    return consumer, build, rustc, host


def collect(upstream, output):
    upstream, output = upstream.resolve(), output.resolve()
    table = json.loads(CONTRACT.read_bytes())
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"expected empty output directory: {output}")
    revision = run(["git", "rev-parse", "HEAD"], upstream).decode().strip()
    if revision != REVISION or revision != table["sourceRevision"]:
        raise ValueError("baseline observation needs the pinned Marmish revision")
    status = run(["git", "status", "--porcelain", "--untracked-files=all"], upstream)
    if status:
        raise ValueError("observation requires a clean upstream tree")
    ids = [c["id"] for c in table["cases"]]
    if not ids or len(ids) != len(set(ids)):
        raise ValueError("expected unique case IDs")
    sources = table["sourceReferences"] + table["release"]["packagingInputs"] + ["Cargo.lock", "rust-toolchain.toml"]
    source_hashes = {k: digest((upstream / k).read_bytes()) for k in sorted(set(sources))}
    packages = tomllib.loads((upstream / "Cargo.lock").read_text())["package"]
    core = next(p for p in packages if p["name"] == "marmish-core")
    codec = next(p for p in packages if p["name"] == "base64" and p["version"] == "0.22.1")
    if ("base64 0.22.1" not in core["dependencies"] or codec["checksum"] !=
            "72b3254f16251a8381aa12e40e3c4d2f0199f8c6508fbecb9d91f575e0fbb8c6"):
        raise ValueError("canonicality argument requires the pinned core base64 implementation")
    env = dict(os.environ, CARGO_NET_OFFLINE="true", RUSTUP_AUTO_INSTALL="0")
    with tempfile.TemporaryDirectory(prefix="marmish-core-observe-") as temp:
        temp = Path(temp)
        consumer, build, rustc, host = build_core_consumer(upstream, temp, env)
        [codec_artifact] = [m for s in build.splitlines() if (m := json.loads(s))["reason"] == "compiler-artifact"
                            and m["target"]["name"] == "base64" and m["package_id"].endswith("#base64@0.22.1")]
        [codec_rlib] = [p for p in codec_artifact["filenames"] if p.endswith(".rlib")]
        tail_source = HERE / "test_base64_tails.rs"
        tail_check = temp / "test-base64-tails"
        run(["rustc", "+1.97.1", "--edition=2021", "--target", host, "--extern", f"base64={codec_rlib}",
             tail_source, "-o", tail_check], upstream, env=env)
        run([tail_check], upstream, env=env)
        results = [observe(consumer, case) for case in table["cases"]]
    output.mkdir(parents=True, exist_ok=True)
    result = {"schemaVersion": 1, "sourceRevision": revision, "contractSha256": digest(CONTRACT.read_bytes()),
              "consumerSha256": digest(CONSUMER.read_bytes()), "rustc": rustc, "host": host,
              "canonicalBase64": {"version": codec["version"], "crateChecksum": codec["checksum"],
                                  "testSha256": digest(tail_source.read_bytes()), "paddedQuartets": 266240},
              "sourceSha256": source_hashes, "corePublicApi": "tested", "cases": results,
              "platforms": {target: ("core baseline observed" if target == host else "unobserved here")
                            for target in table["release"]["targets"]},
              "releaseQualification": "unobserved", "cliObservations": "native/baseline.json"}
    (output / "results.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path)
    parser.add_argument("empty_output", type=Path)
    args = parser.parse_args()
    try:
        collect(args.upstream, args.empty_output)
    except subprocess.CalledProcessError as error:
        sys.exit(error.stderr.decode(errors="replace") or str(error))
    except (OSError, ValueError, KeyError) as error:
        sys.exit(str(error))
