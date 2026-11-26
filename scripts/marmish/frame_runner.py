"""Match exact observed bytes and named errors across all framing implementations."""
import argparse
import io
import json
from pathlib import Path
import subprocess
import sys

from observations import CONTRACT, REVISION, digest, expand

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    return digest(path.read_bytes())


def write(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def verify_inputs(imported, projected, mutant):
    graph = sha(imported / "domain.ttl")
    lock = json.loads((imported / "import.lock").read_bytes())
    if lock["graphSha256"] != graph or lock["profile"] != "framing-v1":
        raise ValueError("import lock disagrees with the framing graph")
    for directory in (projected, mutant):
        manifest = json.loads((directory / "projection.json").read_bytes())
        if manifest["version"] != 1 or manifest["files"] != {"frame.rs": sha(directory / "frame.rs")}:
            raise ValueError("projection does not match its pinned source bytes")
        if (directory == projected) != (manifest["graphSha256"] == graph):
            raise ValueError("expected distinct positive and mutant graphs")
    if sha(projected / "frame.rs") == sha(mutant / "frame.rs"):
        raise ValueError("mutant and production code are identical")
    if sha(projected / "world.wit") != sha(mutant / "world.wit"):
        raise ValueError("mutant changed the public component ABI")


def exact(stream, size):
    data = stream.read(size)
    if len(data) != size:
        raise ValueError("incomplete framing observation")
    return data


def batch(command, cases):
    requests = bytearray()
    for case in cases:
        data = expand(case["input"])
        requests.extend({"encode": b"E", "decode": b"D"}[case["operation"]])
        requests.extend(len(data).to_bytes(4, "little"))
        requests.extend(data)
    output = subprocess.run(command, input=requests, stdout=subprocess.PIPE, check=True, timeout=180)
    stream = io.BytesIO(output.stdout)
    results = []
    for case in cases:
        tag = exact(stream, 1)
        size = int.from_bytes(exact(stream, 4), "little")
        value = exact(stream, size)
        if tag == b"B":
            observation = {"bytesHex": value.hex()}
        elif tag == b"F":
            observation = {"error": value.decode("ascii")}
        else:
            raise ValueError("unknown observation type")
        results.append({"id": case["id"], "operation": case["operation"],
                        "inputSha256": digest(expand(case["input"])), "observed": observation})
    if stream.read(1):
        raise ValueError("unexpected additional observation")
    return results


def compare(out, host, imported, projected, mutant):
    verify_inputs(imported, projected, mutant)
    cases = json.loads(CONTRACT.read_bytes())["cases"]
    original = json.loads((out / "original/results.json").read_bytes())
    if original["sourceRevision"] != REVISION or original["contractSha256"] != sha(CONTRACT):
        raise ValueError("stale original observations")
    build = json.loads((imported / "import.lock").read_bytes())["build"]
    observed_sources = original["sourceSha256"]
    if (build["inputs"]["src/frame.rs"] != observed_sources["crates/marmish-core/src/frame.rs"] or
            build["cargoLockSha256"] != observed_sources["Cargo.lock"]):
        raise ValueError("import and original observations describe different framing source or Cargo.lock")
    expected = []
    for c in cases:
        result = c["expected"]
        observed = {"bytesHex": expand(result["bytes"]).hex()} if "bytes" in result else result
        expected.append({"id": c["id"], "operation": c["operation"],
                         "inputSha256": digest(expand(c["input"])), "observed": observed})
    if original["cases"] != expected:
        raise ValueError("original disagrees with independent expectations")
    builds = {}
    for name, source in (("regenerated", projected), ("mutant", mutant)):
        dest = out / name
        results = {}
        for role, command in (("native", [dest / "consumer"]), ("wasm", [host, dest / "component.wasm"])):
            actual = batch(command, cases)
            differences = [{"case": c["id"], "expected": reference["observed"], "actual": c["observed"]}
                           for reference, c in zip(expected, actual, strict=True) if reference != c]
            required = [] if name == "regenerated" else [
                {"case": "version-before-size", "expected": {"error": "InvalidVersion"},
                 "actual": {"error": "SizeLimit"}}]
            if differences != required:
                raise ValueError(f"{name} {role} unexpected differences: {str(differences)[:300]}")
            path = dest / (role + ".json")
            write(path, {"cases": actual, "differences": differences})
            results[role] = {"observationsSha256": sha(path), "caseCount": len(actual), "differences": differences}
        builds[name] = {
            "projection": {p: sha(source / p) for p in ("projection.json", "frame.rs", "world.wit")},
            "artifacts": {p: sha(dest / p) for p in ("consumer", "component.wasm", "extracted.wit")},
            "wasmImports": [], "results": results,
        }
    src = ROOT / "examples/wasm/framing"
    harness = [p for p in src.rglob("*") if p.is_file()]
    harness += [ROOT / p for p in ("examples/wasm/Cargo.lock", "examples/wasm/host/Cargo.toml",
                                    "examples/wasm/toolchain.lock", "scripts/marmish/frame.sh",
                                    "scripts/marmish/frame_runner.py", "scripts/marmish/observations.py")]
    write(out / "report.json", {"schemaVersion": 1, "sourceRevision": REVISION, "classification": "tested",
          "contractSha256": sha(CONTRACT), "originalSha256": sha(out / "original/results.json"),
          "graphSha256": sha(imported / "domain.ttl"), "importLockSha256": sha(imported / "import.lock"),
          "hostSha256": sha(host), "rustc": (out / "rustc.txt").read_text().strip(),
          "wasmTools": (out / "wasm-tools.txt").read_text().strip(),
          "harnessSha256": {str(p.relative_to(ROOT)): sha(p) for p in sorted(harness)},
          "cases": len(cases), "builds": builds})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("verify-inputs")
    run = commands.add_parser("compare")
    run.add_argument("out", type=Path)
    run.add_argument("host", type=Path)
    for sub in (validate, run):
        sub.add_argument("imported", type=Path)
        sub.add_argument("projected", type=Path)
        sub.add_argument("mutant", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "verify-inputs":
            verify_inputs(args.imported, args.projected, args.mutant)
        else:
            compare(args.out, args.host, args.imported, args.projected, args.mutant)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as exc:
        sys.exit(f"framing verification failed: {exc}")
