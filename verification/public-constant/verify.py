"""Check native rustdoc recovery, projection, and changed-source sensitivity."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).with_name("observatory.rs")


def run(*args, cwd, expected=0, env=None):
    p = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=120)
    if p.returncode != expected:
        raise ValueError(f"{args}: {p.returncode}\n{p.stdout}{p.stderr}")
    return p.stdout + p.stderr


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def native(root, name, ty, value, library):
    caller = root / "caller.rs"
    caller.write_text(
        f"const VALUE: {ty} = observatory::{name};\n"
        f"const _: [(); {value}] = [(); VALUE as usize];\n"
        'fn main() { println!("{}", VALUE); }\n'
    )
    rlib = root / ("lib" + library.stem + ".rlib")
    run("rustc", "+1.97.1", "--edition", "2021", "--crate-name", "observatory", "--crate-type", "rlib",
        library, "-o", rlib, cwd=root)
    exe = root / "caller"
    run("rustc", "+1.97.1", "--edition", "2021", caller, "--extern", f"observatory={rlib}", "-o", exe, cwd=root)
    assert run(exe, cwd=root) == f"{value}\n"


def fixture(root, source_text, expected):
    source = root / "observatory.rs"
    source.write_text(source_text)
    run("rustdoc", "+1.97.1", "-Z", "unstable-options", "--output-format", "json",
        "--crate-name", "observatory", source.name, cwd=root, env=dict(os.environ, RUSTC_BOOTSTRAP="1"))
    doc = root / "doc" / "observatory.json"

    def select(name, output, observed=doc, error=None):
        selector = root / "selection.json"
        term = "urn:observatory:limits:" + name
        selector.write_text(json.dumps({
            "format": "oddlang-public-constant-selection-v1",
            "id": "urn:observatory:limits",
            "constantId": term,
            "crateName": "observatory",
            "name": name,
            "sourceFilename": source.name,
            "sourceSha256": digest(source),
            "rustdocSha256": digest(observed),
        }))
        message = run(CLI, "recover", "public-constant", source, observed, selector, output,
                      expected=1 if error else 0, cwd=root)
        if error:
            assert error in message and not output.exists(), message
        return term

    results = {}
    for name, (ty, value, expression) in expected.items():
        selection = root / name
        term = select(name, selection)
        repeat = root / (name + "-repeat")
        select(name, repeat)
        for artifact in ("graph.json", "rust.json", "recovery.lock"):
            assert (selection / artifact).read_bytes() == (repeat / artifact).read_bytes()
        graph = json.loads((selection / "graph.json").read_text())
        assert graph["constants"] == [{"id": term, "value": expression}]
        lock = json.loads((selection / "recovery.lock").read_text())
        assert lock["unsignedDecimal"] == str(value)
        assert lock["graphSha256"] == digest(selection / "graph.json")
        projected = root / ("projection-" + name)
        run(CLI, "build", "shared-declarations", "rust", selection / "graph.json",
            selection / "rust.json", projected, cwd=root)
        native(root, name, ty, value, source)
        native(root, name, ty, value, projected / "shared.rs")
        results[name] = graph

    select("PRIVATE_LIMIT", root / "private", error="expected exactly one selected local root child")
    return doc, select, results


with tempfile.TemporaryDirectory(prefix="oddlang-constant-") as stage:
    stage = Path(stage)
    a, b = stage / "baseline", stage / "mutation"
    a.mkdir()
    b.mkdir()
    source = FIXTURE.read_text()
    doc, _, baseline = fixture(a, source, {
        "GRID_LIMIT": ("u16", 105, {"multiply": [{"multiply": [3, 5]}, 7]}),
        "SAMPLE_COUNT": ("usize", 42, 42),
    })
    _, select, mutated = fixture(b, source.replace(" * 7;", " * 8;"), {
        "GRID_LIMIT": ("u16", 120, {"multiply": [{"multiply": [3, 5]}, 8]}),
    })
    assert baseline["GRID_LIMIT"] != mutated["GRID_LIMIT"]
    select("GRID_LIMIT", b / "stale", observed=doc, error="disagrees with compiler evaluation")
print("Public constants: native compiler source/value binding, product mutation, and projected caller replay passed")
