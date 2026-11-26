#!/usr/bin/env python3
"""Qualify generated copy Data encoders, decoders and incremental consumers."""
import argparse
from collections import Counter
import json
import platform
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

import run as oracle
from spec_cases import case, error, frame, invalid


RELEASE_PIN = "82ddfed871221091be6b42adf6d0b009cb5ce10f864c414bdbaed5e19e4dad56"
TAG = 5
CHANGED_TAG = 51
ROOT = Path(__file__).resolve().parent
GRAPH = ROOT.parent / "portable-contract" / "data.ttl"
HINTS = GRAPH.with_name("hints.json")
SCOPE = "urn:marmish:portable:copy-frame:v1:Data"
SYSTEM_READS = (
    "/System/Library", "/usr/lib", "/usr/bin", "/bin", "/Library/Apple", "/private/var/db/dyld",
    "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
    "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
)
SANDBOX = "/usr/bin/sandbox-exec"


def checked(command, cwd, environment, data=None, timeout=60):
    result = subprocess.run(command, cwd=cwd, env=environment, input=data, timeout=timeout,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise ValueError(f"{shlex.join(map(str, command[:3]))} exited {result.returncode}: "
                         f"{result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def boundary(stage, read=(), executable=()):
    subpaths = " ".join(f"(subpath {json.dumps(str(path))})" for path in (*SYSTEM_READS, stage, *read))
    literals = " ".join(f"(literal {json.dumps(str(path))})" for path in (*executable, "/", "/dev/null", "/dev/urandom"))
    root = json.dumps(str(stage))
    return [SANDBOX, "-p", f"""(version 1)
(deny default)
(allow process-fork process-exec file-read-metadata sysctl-read signal)
(allow process-info* (target self))
(allow file-read-data file-map-executable {subpaths} {literals})
(allow file-write* (subpath {root}) (literal "/dev/null"))
"""]


def environment(stage, sdk=None):
    values = {"PATH": "/usr/bin:/bin", "HOME": str(stage), "TMPDIR": str(stage),
              "LANG": "C", "LC_ALL": "C", "TZ": "UTC"}
    if sdk:
        values["SDKROOT"] = str(sdk)
    return values


def probe_denied(prefix, paths, stage):
    results = []
    for role, path in paths.items():
        oracle.require(path.is_file(), f"withheld input {role} must exist")
        variants = [(role, path)]
        alias = Path("/System/Volumes/Data" + str(path))
        if alias.is_file():
            variants.append((role + "ThroughDataVolume", alias))
        for label, spelling in variants:
            result = subprocess.run([*prefix, "/bin/cat", str(spelling)], cwd=stage, env=environment(stage),
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
            oracle.require(result.returncode == 1 and not result.stdout
                           and b"Operation not permitted" in result.stderr, f"could read withheld input {label}")
            results.append(label)
    return sorted(results)


def run_rows(command, requests, stage, env):
    data = ("\n".join(requests) + "\n").encode("ascii")
    result = subprocess.run(command, cwd=stage, env=env, input=data, timeout=30,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    oracle.require(result.returncode == 0 and result.stderr == b"", "target adapter failed or wrote stderr")
    rows = result.stdout.decode("ascii").splitlines()
    oracle.require(len(rows) == len(requests), "missing or unexpected target adapter observations")
    return rows


def build(stage, oddlang, graph, denied, approved_pin=None):
    stage.mkdir()
    builder = stage / "oddlang"
    shutil.copy2(oddlang, builder)
    contract = stage / "data.ttl"
    contract.write_bytes(graph)
    projection = boundary(stage)
    controls = probe_denied(projection, denied, stage)
    env = environment(stage)
    pin = checked([*projection, str(builder), "tagged-wire", "digest", str(contract)], stage, env).decode().strip()
    oracle.require(re.fullmatch("[0-9a-f]{64}", pin) is not None, "invalid normative graph digest")
    if approved_pin is not None:
        oracle.require(pin == approved_pin, f"release graph digest mismatch: {pin}")
    commands = {}
    outputs = {}
    locks = {}
    for target, artifact in (("rust", "wire.rs"), ("typescript", "wire.mts")):
        out = stage / target
        command = ["build", "tagged-wire", target, str(contract), str(out), "--graph-sha256", pin]
        checked([*projection, str(builder), *command], stage, env)
        lock = out / "projection.lock"
        oracle.require(json.loads(lock.read_bytes()) == {
            "vocabulary": "oddlang-tagged-wire-v1", "graphSha256": pin, "target": target, "source": artifact,
        }, f"projection lock mismatch for {target}")
        locks[target] = oracle.digest(lock)
        outputs[target] = oracle.digest(out / artifact)
        commands[target] = [arg.replace(str(stage), "$STAGE") for arg in command]
    return {"graphSha256": pin, "graphFileSha256": oracle.digest(contract),
            "generatorSha256": oracle.digest(builder), "generatedSourceSha256": outputs,
            "projectionLockSha256": locks,
            "deniedReadProbes": controls, "commandArguments": commands,
            "sandboxProfileSha256": oracle.sha(projection[-1].replace(str(stage), "$STAGE").encode()).hex()}


def compile_adapters(stage, rustc, node, developer, sdk, clang, denied, build_evidence):
    rust_dir, node_dir = stage / "rust", stage / "typescript"
    sources = {"rust": rust_dir / "wire.rs", "typescript": node_dir / "wire.mts"}

    def verify_sources():
        oracle.require({target: oracle.digest(path) for target, path in sources.items()}
                       == build_evidence["generatedSourceSha256"], "generated source changed before replay")

    verify_sources()
    shutil.copy2(ROOT / "tagged_adapter.rs", rust_dir / "adapter.rs")
    shutil.copy2(ROOT / "tagged_adapter.mjs", node_dir / "adapter.mjs")
    executable = rust_dir / "adapter"
    native = boundary(stage, read=[rustc.parent.parent, developer])
    rust_probes = probe_denied(native, denied, stage)
    sdk_env = environment(stage, sdk)
    checked([*native, str(rustc), "--edition=2024", "-C", f"linker={clang}", str(rust_dir / "adapter.rs"), "-o", str(executable)],
            stage, sdk_env)
    node_runtime = boundary(stage, executable=[node])
    node_probes = probe_denied(node_runtime, denied, stage)
    node_cmd = [
        *node_runtime, str(node), "--no-warnings", "--experimental-strip-types",
        str(node_dir / "adapter.mjs"),
    ]
    verify_sources()
    build_evidence["adapterBoundaries"] = {
        role: {"deniedReadProbes": probes,
               "sandboxProfileSha256": oracle.sha(profile[-1].replace(str(stage), "$STAGE").encode()).hex()}
        for role, profile, probes in [("rustCompile", native, rust_probes), ("nodeRuntime", node_runtime, node_probes)]
    }
    return {"rust": [*boundary(stage), str(executable)], "typescript": node_cmd}


def comparable(row):
    operation = row["operation"]
    expected = row["expected"]
    if operation == "frame_encode":
        return row["input"]["kind"] == "data"
    if operation == "frame_decode":
        raw = bytes.fromhex(oracle.expand(row["input"]))
        return len(raw) < 5 or raw[0] == TAG or raw[0] not in range(1, 12)
    if operation == "frame_fragments":
        return all(not isinstance(step, dict) or step.get("frame") is None or step["frame"]["kind"] == "data"
                   for step in expected)
    return False


def supplemental_cases():
    packet = frame(TAG, b"\0" * 8 + b"\xff")
    shape = {"kind": "data", "offset": 0, "bytes": "ff"}
    steps = [{"consumed": 1, "frame": None} for _ in packet[:-1]]
    steps.extend([{"consumed": 1, "frame": shape}, "ok"])
    return [
        case("tagged-short-offset", "frame_decode", frame(TAG, b"\0" * 7).hex(), invalid("copy_truncated")),
        case("tagged-control-cap-minus-one", "frame_decode", "ff00003fff", invalid("copy_frame_length")),
        case("tagged-control-cap", "frame_decode", "ff00004000", invalid("copy_frame_length")),
        case("tagged-data-payload-minus-one", "frame_encode", {
            "kind": "data", "offset": 0, "bytes": {"prefix": "", "byte": 255, "repeat": 8191},
        }, {"prefix": "05000020070000000000000000", "byte": 255, "repeat": 8191}),
        case("tagged-encode-8193-valid-offset", "frame_encode", {
            "kind": "data", "offset": 0, "bytes": {"prefix": "", "byte": 255, "repeat": 8193},
        }, error("Limit")),
        case("tagged-encode-8193-invalid-offset", "frame_encode", {
            "kind": "data", "offset": 2**64 - 1, "bytes": {"prefix": "", "byte": 255, "repeat": 8193},
        }, error("Limit")),
        case("tagged-encode-u64-max", "frame_encode",
             {"kind": "data", "offset": 2**64 - 1, "bytes": "ff"}, error("Offset")),
        case("tagged-decode-u64-max", "frame_decode", frame(TAG, b"\xff" * 9).hex(), error("Offset")),
        case("tagged-data-length-mismatch", "frame_decode", packet[:-1].hex(), invalid("copy_frame_length")),
        case("tagged-bytewise-stream", "frame_fragments", [*[f"{b:02x}" for b in packet], "finish"], steps),
        case("tagged-latched-cap", "frame_fragments", ["ff00004001", packet.hex(), "finish"], [
            error("Limit"), invalid("copy_decoder_failed"), invalid("copy_truncated"),
        ]),
    ]


def case_inputs(case):
    data = oracle.expand(case["input"])
    operation = case["operation"]
    if operation == "frame_encode":
        return [f"E {data['offset']} {data['bytes']}"]
    if operation == "frame_decode":
        return [f"D {data}"]
    return ["B", *["X" if step == "finish" else f"F {step}" for step in data]]


def result_line(step, success):
    if isinstance(step, dict) and "error" in step:
        detail = "/" + step["detail"] if "detail" in step else ""
        return f"!{step['error']}{detail}"
    return success(step)


def record(step):
    oracle.require(step["kind"] == "data", "non-Data record in selected oracle")
    return f"{TAG}:{step['offset']}:{step['bytes']}"


def expected(case):
    value = oracle.expand(case["expected"])
    operation = case["operation"]
    if operation == "frame_encode":
        return [result_line(value, lambda data: "E:" + data)]
    if operation == "frame_decode":
        return [result_line(value, lambda data: "D:" + record(data))]

    def feed(step):
        if step == "ok":
            return "X"
        frame = record(step["frame"]) if step["frame"] is not None else ""
        return f"F:{step['consumed']}:{frame}"

    return ["B", *[result_line(step, feed) for step in value]]


def assert_observations(stage, command, cases, *, sealed=False):
    try:
        requests = [request for case in cases for request in case_inputs(case)]
        observations = run_rows(command, requests, stage, environment(stage))
        offset = 0
        for case in cases:
            result = expected(case)
            wanted = observations[offset:offset + len(result)]
            oracle.require(wanted == result, f"mismatch for {case['name']}")
            offset += len(result)
        oracle.require(offset == len(observations), "extra observations")
    except (ValueError, OSError, subprocess.SubprocessError):
        if sealed:
            raise ValueError("sealed recovered target replay failed") from None
        raise
    return observations


def transferable(cases):
    return [row for row in cases if row["operation"] == "frame_encode" and "error" not in row["expected"]]


def peer_read_cases(cases, packets):
    return [case(f"peer:{row['name']}", "frame_decode", packet, oracle.expand(row["input"]))
            for row, packet in zip(cases, packets, strict=True)]


def record_exchange(packets, observations):
    return {"records": len(packets), "wireSha256": oracle.corpus_digest(packets),
            "decodedRowsSha256": oracle.corpus_digest(observations)}


def generated_writes(stage, adapters, cases):
    inputs = [case_inputs(row)[0] for row in cases]
    outputs = {}
    for name, command in adapters.items():
        responses = run_rows(command, inputs, stage, environment(stage))
        oracle.require(responses == [expected(row)[0] for row in cases], "peer writer bytes differ")
        outputs[name] = [response.removeprefix("E:") for response in responses]
    return outputs


def compare_original(upstream, stage, public, holdout, holdout_path, mutations):
    command, provenance = oracle.build_original(upstream, stage)
    binding = provenance["callerBinding"]
    observations = oracle.observe(command, public, binding)
    oracle.require(not oracle.failures(public, observations), "original failed public expectations")
    if holdout:
        try:
            isolated = [*oracle.holdout_boundary(holdout_path), *command]
            result = oracle.observe(isolated, holdout, binding)
            oracle.require(not oracle.failures(holdout, result), "original failed sealed expectations")
        except (ValueError, OSError, subprocess.SubprocessError):
            raise ValueError("sealed original replay failed") from None
    oracle.require(not oracle.failures(mutations, oracle.observe(command, mutations, binding)),
                   "original accepted a changed Data tag")
    return command, provenance


def peers(stage, adapters, cases, original, binding):
    cases = transferable(cases)
    outputs = generated_writes(stage, adapters, cases)
    original_writes = oracle.observe(original, cases, binding)
    oracle.require(not oracle.failures(cases, original_writes), "upstream writer bytes differ")
    outputs["upstream"] = [row["actual"] for row in original_writes]
    report = {}
    for writer, packets in outputs.items():
        reads = peer_read_cases(cases, packets)
        for reader in outputs.keys() - {writer}:
            if reader == "upstream":
                actual = oracle.observe(original, reads, binding)
                oracle.require(not oracle.failures(reads, actual), "upstream peer rejected generated data")
                decoded = ["D:" + record(row["actual"]) for row in actual]
            else:
                decoded = assert_observations(stage, adapters[reader], reads)
            report[f"{writer}To{reader.title()}"] = record_exchange(packets, decoded)
    return report


def mutation_results(release, alternate, adapters, mutants, sample):
    output = {}
    original = expected(sample)[0].removeprefix("E:")
    mutated = f"{CHANGED_TAG:02x}{original[2:]}"
    decoded = f"{CHANGED_TAG}:{sample['input']['offset']}:{sample['input']['bytes']}"
    offset, payload = sample["input"]["offset"], sample["input"]["bytes"]
    unknown = "!Invalid/copy_frame_tag"
    invalid_value = "!Invalid/value"
    for name, command in mutants.items():
        requests = [
            case_inputs(sample)[0], f"D {original}", f"D {mutated}",
            "B", f"F {mutated[:8]}", f"F {mutated[8:]}", "X",
            "B", f"F {original}", f"F {mutated}", "X",
            f"T {TAG} {offset} {payload}",
        ]
        expected_rows = [
            f"E:{mutated}", unknown, f"D:{decoded}",
            "B", "F:4:", f"F:{len(mutated) // 2 - 4}:{decoded}", "X",
            "B", unknown, "!Invalid/copy_decoder_failed", "!Invalid/copy_truncated",
            invalid_value,
        ]
        oracle.require(run_rows(command, requests, alternate, environment(alternate)) == expected_rows,
                       f"{name} graph tag mutation encode/decode/stream mismatch")
        oracle.require(run_rows(adapters[name], [f"T {CHANGED_TAG} {offset} {payload}"], release,
                                environment(release)) == [invalid_value], f"{name} accepted wrong encode tag")
        output[name] = case(f"{name}-changed-tag", "frame_decode", mutated, invalid("copy_frame_tag"))
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oddlang", type=Path, required=True)
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--graph", type=Path, default=GRAPH)
    parser.add_argument("--hints", type=Path, help="advisory file withheld from target generation")
    parser.add_argument("--holdout", type=Path)
    parser.add_argument("--deny-read", action="append", type=Path, default=[])
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    target = shutil.which("node")
    oracle.require(target is not None, "Node is required to run generated TypeScript")
    node = Path(target).resolve()
    rustc = Path(subprocess.check_output(["rustup", "which", "--toolchain", "1.97.1", "rustc"],
                                       text=True).strip()).resolve()
    developer = Path(subprocess.check_output(["xcode-select", "-p"], text=True).strip()).resolve()
    clang = Path(subprocess.check_output(["xcrun", "--find", "clang"], text=True).strip()).resolve()
    sdk = Path(subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()).resolve()
    upstream = args.upstream.resolve()
    freeze = json.loads(oracle.FREEZE.read_bytes())
    prior = json.loads(oracle.VECTORS.read_bytes())
    spec = oracle.spec_cases()
    seeded = oracle.seeded_cases(freeze["seed"], freeze["seededRounds"])
    oracle.require(oracle.digest(oracle.VECTORS) == freeze["priorCorpusSha256"]
                   and oracle.corpus_digest(spec) == freeze["authoredCorpusSha256"]
                   and oracle.corpus_digest(seeded) == freeze["seededCorpusSha256"],
                   "phase-one oracle corpus pin mismatch")
    public = [row for row in prior["cases"] + spec + seeded if comparable(row)] + supplemental_cases()
    holdout = [row for row in oracle.sealed_cases(args.holdout, freeze) if comparable(row)] if args.holdout else []
    pin = args.graph.read_bytes()
    changed, mutations = re.subn(rb"(t:tag )5( ;)", rb"\g<1>51\2", pin)
    oracle.require(mutations == 1, "graph must have exactly one expected Data tag")
    oddlang = args.oddlang.resolve()

    with tempfile.TemporaryDirectory(prefix="oddlang-qualify-copy-data-") as temp:
        work = Path(temp).resolve()
        withheld = work / "withheld"
        withheld.mkdir()
        hint_input = args.hints or (HINTS if HINTS.is_file() else None)
        hints = None
        hint_evidence = None
        if hint_input:
            hint_input = hint_input.resolve()
            hints = withheld / "advisory-hints"
            shutil.copy2(hint_input, hints)
            hint_evidence = {"file": hint_input.name, "sha256": oracle.digest(hints)}
            if hint_input == HINTS:
                lock = HINTS.with_suffix(".sha256").read_text().split()
                oracle.require(lock == [hint_evidence["sha256"], hint_input.name], "advisory hint digest mismatch")
        private = withheld / "structure.ttl"
        private.write_text('<urn:withheld:source> <urn:withheld:edge> <urn:withheld:callee> .\n')
        denied = {
            "upstreamSource": upstream / "crates/marmish-stream/src/copy_protocol.rs",
            "oracleVectors": oracle.VECTORS,
            "withheldStructureProbe": private,
        }
        withheld_inputs = {}
        for index, path in enumerate(args.deny_read):
            path = path.resolve()
            label = f"withheldInput{index}"
            denied[label] = path
            withheld_inputs[label] = {
                "role": "privateStructureGraph" if path.name == "structure.ttl" else "withheldInput",
                "file": path.name,
                "sha256": oracle.digest(path),
            }
        if args.holdout:
            denied["sealedCapsule"] = args.holdout.resolve()
        release, without_hints, alternate = [work / name for name in ("release", "no-hints", "changed-tag")]
        if hint_input is not None:
            denied["advisoryHintSource"] = hint_input
        initial_denied = denied | {"withheldHintCopy": hints} if hints is not None else denied
        original_build = build(release, oddlang, pin, initial_denied, RELEASE_PIN)
        if hints is not None:
            hints.unlink()
            hint_evidence["withheldOnFirstBuild"] = True
        changed_build = build(alternate, oddlang, changed, denied)
        no_hints_build = build(without_hints, oddlang, pin, denied, RELEASE_PIN)
        oracle.require(original_build["graphSha256"] == no_hints_build["graphSha256"]
                       and original_build["generatedSourceSha256"] == no_hints_build["generatedSourceSha256"],
                       "withholding optional hints changed generation")
        oracle.require(changed_build["graphSha256"] != original_build["graphSha256"], "mutation did not change graph pin")
        adapters = compile_adapters(release, rustc, node, developer, sdk, clang, denied, original_build)
        mutant = compile_adapters(alternate, rustc, node, developer, sdk, clang, denied, changed_build)
        restored = compile_adapters(without_hints, rustc, node, developer, sdk, clang, denied, no_hints_build)
        compared = {name: len(assert_observations(release, command, public)) for name, command in adapters.items()}
        if holdout:
            # The target boundary already denies all reads outside the staging and runtime tools.
            for command in adapters.values():
                assert_observations(release, command, holdout, sealed=True)
        witness = next(row for row in public if row["name"] == "binary-data-encode")
        output = mutation_results(release, alternate, adapters, mutant, witness)
        original_command, original = compare_original(
            upstream, work / "upstream-build", public, holdout, args.holdout, list(output.values()),
        )
        mixed = peers(release, adapters, public, original_command, original["callerBinding"])
        # Recheck the unmodified contract after both mutation controls.
        for command in restored.values():
            assert_observations(without_hints, command, public)
        if hint_evidence is not None:
            hint_evidence["consumedByBuilder"] = False
            hint_evidence["withheldCopyPresenceOrAbsencePreservedOutput"] = True
        receipt = {
            "schema": 1, "contract": SCOPE, "approvedGraphSha256": RELEASE_PIN,
            "additionalWithheldInputs": withheld_inputs,
            "upstreamRevision": oracle.UPSTREAM, "freezeSha256": oracle.digest(oracle.FREEZE),
            "publicCases": dict(Counter(row["operation"] for row in public)), "publicCount": len(public),
            "publicCaseIds": [row["name"] for row in public],
            "sealedCases": len(holdout), "comparedOperationResults": compared,
            "driverSourceSha256": {name: oracle.digest(ROOT / name) for name in [
                "qualify_tagged.py", "tagged_adapter.rs", "tagged_adapter.mjs",
            ]},
            "mixedPeers": mixed, "build": original_build, "hintOmission": hint_evidence,
            "alternativeLibraryValidation": "unresolved",
            "buildWithoutHints": no_hints_build, "mutatedBuild": changed_build,
            "mutatedOutputs": {name: row["input"] for name, row in output.items()},
            "mutation": {"from": TAG, "to": CHANGED_TAG, "upstreamRejected": sorted(output),
                         "mutantsDecodedOwnWire": sorted(output), "mutantsFedOwnWire": sorted(output),
                         "mutantsRejectedOriginalDecodeAndFeed": sorted(output),
                         "wrongEncodeTagsRejected": sorted(output), "restoredPinReplayed": True},
            "originalCaller": original,
            "runtime": {
                "host": platform.platform(),
                "rust": subprocess.check_output([str(rustc), "-vV"], text=True).splitlines(),
                "rustcSha256": oracle.digest(rustc),
                "clangSha256": oracle.digest(clang),
                "sdkSettingsSha256": oracle.digest(sdk / "SDKSettings.json"),
                "node": subprocess.check_output([str(node), "--version"], text=True).strip(),
                "nodeSha256": oracle.digest(node),
                "nodeOptions": ["--no-warnings", "--experimental-strip-types"],
            },
            "gaps": [
                "Copy variants other than Data and any mixed frames requiring another variant",
                "Windows, codec, stream JSON wire, bridge command and host effects",
                "Full Rust public paths, signatures, traits, bindings and native consumers",
                "Invalid portable values and trailing payload are outside the selected upstream-comparison obligations",
                "Successful filesystem reads were confined, and file metadata probes remain allowed",
                "Native runtime dependencies and platform profiles beyond those recorded",
                "Alternative compression library role is outside the Data contract",
            ],
        }
    oracle.write(args.receipt, receipt)
    print(f"{len(public)} public cases; {len(holdout)} sealed cases; both native targets and graph tag mutation passed")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
