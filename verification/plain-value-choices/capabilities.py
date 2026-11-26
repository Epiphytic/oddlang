"""Exercise the clone-only and derived Debug extension with independent Rust witnesses."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

COMPILER = Path(sys.argv[1]).resolve()
RUST = ["rustc", "+1.97.1", "--edition=2024"]
DOMAIN = "urn:fixture:route:choice"
BASE = "#[derive(Clone, Debug, PartialEq, Eq)]\npub enum Bearing { East, West }\n"
CHANGE = "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum Bearing { West, East }\n"
CALLER = '''
use test_sample::Bearing;
fn main() {
    let first = Bearing::East;
    let second = Bearing::West;
    assert_eq!(first, first.clone());
    assert_ne!(first, second);
    println!("{first:?}={} {second:?}={}", first.clone() as isize, second.clone() as isize);
}
'''


def run(*command, check=True):
    out = subprocess.run(command, capture_output=True, timeout=60)
    if check and out.returncode:
        raise RuntimeError(out.stderr.decode())
    return out


def selection(path, data, version="v2"):
    path.write_text(json.dumps({"format": f"oddlang-plain-choice-selection-{version}",
                                "id": DOMAIN, "name": "Bearing",
                                "sourceSha256": hashlib.sha256(data.encode()).hexdigest()}))


def observe(work, source, accepts_copy):
    lib = work / "libtest_sample.rlib"
    run(*RUST, "--crate-name", "test_sample", "--crate-type", "lib", source, "-o", lib)
    caller = work / "caller.rs"
    caller.write_text(CALLER)
    args = [*RUST, "--extern", f"test_sample={lib}", "--crate-name", "caller"]
    binary = work / "caller"
    run(*args, caller, "-o", binary)
    output = run(binary).stdout
    caller.write_text("fn copy<T: Copy>() {} fn main() { copy::<test_sample::Bearing>(); }")
    copy = run(*args, "--error-format=json", caller, "-o", binary, check=False)
    if accepts_copy:
        assert copy.returncode == 0, copy.stderr
    else:
        codes = [row["code"]["code"] for line in copy.stderr.splitlines()
                 if (row := json.loads(line)).get("code") and row["level"] == "error"]
        assert copy.returncode != 0 and codes == ["E0277"], copy.stderr
    return output


with tempfile.TemporaryDirectory(prefix="odd-choice-capabilities-") as tmp:
    work = Path(tmp)
    observed = []
    for label, text in (("baseline", BASE), ("mutation", CHANGE)):
        directory = work / label
        directory.mkdir()
        source = directory / "choices.rs"
        selector = directory / "selection.json"
        source.write_text(text)
        selection(selector, text)
        declaration = directory / "recovered"
        run(COMPILER, "recover", "plain-choice", source, selector, declaration)
        graph = json.loads((declaration / "graph.json").read_bytes())
        [definition] = graph["types"].values()
        accepts_copy = label == "mutation"
        assert definition.get("copy", True) == accepts_copy and definition["debug"] is True
        product = directory / "product"
        run(COMPILER, "build", "checked-values", "rust", declaration / "graph.json",
            declaration / "rust.json", product)
        [obligation] = json.loads((product / "obligations.json").read_bytes())["obligations"]
        assert obligation["expected"]["copy"] == accepts_copy
        assert obligation["expected"]["debug"] == "derived-variant-name"
        assert obligation["expected"]["externalRepresentation"] is None
        original = observe(directory, source, accepts_copy)
        generated = observe(directory, product / "values.rs", accepts_copy)
        assert original == generated
        observed.append(original)
        definition["copy"] = not accepts_copy
        changed = directory / "bad.json"
        changed.write_text(json.dumps(graph))
        failure = run(COMPILER, "build", "checked-values", "rust", changed,
                      declaration / "rust.json", directory / "bad", check=False)
        assert failure.returncode and b"invalid plain choice native binding" in failure.stderr
    assert observed == [b"East=0 West=1\n", b"East=1 West=0\n"]
    source, selector = work / "unsupported.rs", work / "select.json"
    for text, version in (
        (BASE, "v1"), (BASE.replace("Debug", "Debug, Serialize"), "v2"),
        ("use custom::Debug;\n" + BASE, "v2"),
    ):
        source.write_text(text)
        selection(selector, text, version)
        result = run(COMPILER, "recover", "plain-choice", source, selector, work / "bad", check=False)
        assert result.returncode and not (work / "bad").exists()
print("Both native variants match exact Debug, Eq, Clone, Copy and declaration ordinals.")
