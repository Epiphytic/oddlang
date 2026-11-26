"""Collect controlled black-box traces from real, independently written native buffers."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
FRAMEWORK = HERE.parents[1]
CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1", CARGO_NET_OFFLINE="true")
BASELINE = (HERE / "reference.rs").read_text()
PROBE = (HERE / "probe.rs").read_text()


def run(*args, cwd, env=ENV, error=None):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=240)
    stderr = result.stderr.decode(errors="replace")
    if error is None:
        assert not result.returncode, stderr
    else:
        assert result.returncode and error in stderr, (args, stderr)
    return result.stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, data):
    path.write_text(json.dumps(data, indent=2) + "\n")
    return path


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    source = root / "src"
    source.mkdir()
    lib = source / "lib.rs"
    manifest = root / "Cargo.toml"
    manifest.write_text('[package]\nname="field-scanner"\nversion="0.1.0"\n'
                        'edition="2021"\n[dependencies]\nzeroize="=1.9.1"\n')
    lib.write_text(BASELINE)
    run("cargo", "generate-lockfile", "--offline", cwd=root)
    host = next(line[6:] for line in run("rustc", "-vV", cwd=root).decode().splitlines()
                if line.startswith("host: "))
    selection = write(root / "selection.json", {
        "declaration": "local::field_scanner::Scanner::{type}",
        "id": "urn:oddlang:example:scanner-buffer",
        "fields": dict(input="source", storage="block", begin="begin", end="end"),
    })

    def collect(crate, output):
        result = run("cargo", "test", "--locked", "--offline", "--quiet", "--target-dir",
                     root / "native-tests", cwd=crate, env=dict(ENV, ODD_BUFFER_TRACE=str(output)))
        assert b"1 passed" in result
        return output

    def shape(name):
        path = root / f"{name}.observation.json"
        path.write_bytes(run("bash", FRAMEWORK/"verification/rust-recovery/run.sh",
                             manifest, "field-scanner", "lib", "field_scanner", host, "default",
                             cwd=root, env=dict(ENV, ODD_RECOVERY_DATA_SHAPES="1")))
        return path

    def infer(observation, trace, out, *, error=None, selected=selection):
        run(CLI, "recover", "observed-buffered-input", observation, selected, trace, out,
            cwd=root, error=error)
        assert out.exists() != bool(error)
        return out

    mutated = BASELINE.replace("const EARLY_EMPTY: bool = false;", "const EARLY_EMPTY: bool = true;")
    mutated = mutated.replace("const DISCARD_ON_ERROR: bool = false;",
                              "const DISCARD_ON_ERROR: bool = true;")
    mutated = mutated.replace("const ERASE_CONSUMED: bool = true;", "const ERASE_CONSUMED: bool = false;")
    traces = []
    profiles = [
        ("baseline", BASELINE, ("refill-if-exhausted", "reset-begin-retain-end", "erase")),
        ("mutated", mutated, ("return-zero", "discard-window", "retain")),
    ]
    for name, implementation, expected in profiles:
        lib.write_text(implementation + PROBE)
        observed = shape(name)
        captured = collect(root, root / f"{name}.json")
        bundle = infer(observed, captured, root/name)
        graph = json.loads((bundle / "graph.json").read_bytes())
        assert graph["capacity"] == 8 and graph["fieldOrder"] == ["input", "storage", "begin", "end"]
        assert tuple(graph["transitions"][k] for k in ("emptyRead", "refillError", "consumedBytes")) == expected
        lock = json.loads((bundle / "recovery.lock").read_bytes())
        assert lock["traceSha256"] == digest(captured)
        assert lock["observationSha256"] == digest(observed)
        assert lock["inferred"] == ["empty-read policy", "refill-error policy", "consumed-byte policy"]
        assert len(lock["providerAssumptions"]) >= 3
        same = infer(observed, captured, root/f"{name}-repeat")
        assert all(p.read_bytes() == (same/p.name).read_bytes() for p in bundle.iterdir())
        traces.append((bundle, captured, observed, graph))

    lib.unlink()
    assert not list(source.glob("*.rs"))
    assert {k:v for k,v in traces[0][3].items() if k != "transitions"} == {
        k:v for k,v in traces[1][3].items() if k != "transitions"}
    assert (traces[0][0] / "rust.json").read_bytes() == (traces[1][0] / "rust.json").read_bytes()
    for bundle, recorded, _, _ in traces:
        package = write(root / "package.json", {
            "format":"oddlang-native-package-v1", "name":"field-scanner", "version":"0.1.0",
            "edition":"2021", "dependencies":{"zeroize":{"version":"1.9.1"}},
            "cargoLock":{"path":"Cargo.lock","sha256":digest(root/"Cargo.lock")},
            "components":[{"kind":"buffered-input","module":"scanner","reexports":[],
                           "graph":{"path":f"{bundle.name}/graph.json","sha256":digest(bundle/"graph.json")},
                           "overlay":{"path":f"{bundle.name}/rust.json","sha256":digest(bundle/"rust.json")}}],
        })
        output = root / f"candidate-{bundle.name}"
        run(CLI, "build", "native-package", "rust", package, output, cwd=root)
        with (output / "src/group_0/__projection_0.rs").open("a") as leaf:
            leaf.write(PROBE)
        result = collect(output, root/f"candidate-{bundle.name}.json")
        assert json.loads(result.read_bytes()) == json.loads(recorded.read_bytes())

    existing, captured, observation, _ = traces[0]
    before = {path.name: digest(path) for path in existing.iterdir()}
    run(CLI, "recover", "observed-buffered-input", observation, selection, captured, existing,
        cwd=root, error="publish fresh package")
    assert before == {path.name: digest(path) for path in existing.iterdir()}
    wrong_selector = write(root/"wrong-selection.json", json.loads(selection.read_bytes()) | {
        "transitions": traces[0][3]["transitions"],
    })
    infer(observation, captured, root/"authored-during-inference",
          error="unknown field", selected=wrong_selector)

    trace = json.loads(captured.read_bytes())
    invalid = [
        ("emptyRead", "payload", [0, 0], "probe payload"),
        ("emptyRead", "callsAfterEmpty", 2, "empty-read evidence"),
        ("emptyRead", "afterFill", [], "empty-read evidence"),
        ("consume", "requested", 4, "partial-consume evidence"),
        ("consume", "nextReadPrefix", [0, 0, 0, 1], "retirement evidence"),
        ("consume", "afterRefill", [7], "partial-consume evidence"),
        ("refillError", "returnedError", False, "refill-error evidence"),
        ("refillError", "afterRetry", trace["refillError"]["payload"], "refill-error evidence"),
        ("refillError", "callsAfterRetry", 4, "refill-error evidence"),
        ("consume", "payload", list(range(1, 10)), "probe payload"),
    ]
    for i, (section, key, value, error) in enumerate(invalid):
        changed = copy.deepcopy(trace)
        changed[section][key] = value
        malformed = write(root / f"bad-{i}.json", changed)
        infer(observation, malformed, root/f"rejected-{i}", error=error)
    print("Real shape-invariant Rust mutation changed all three inferred policies; both native projections reproduce their traces.")
