"""Run an independent Rust witness and a source-free diagnostic library."""
import errno
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

FIXTURE = Path(__file__).resolve().parent
COMPILER = Path(sys.argv[1]).resolve()


def run(*arguments, cwd=None, check=True, stderr=subprocess.PIPE):
    result = subprocess.run(arguments, cwd=cwd, check=False, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=stderr, timeout=120)
    if check and result.returncode != 0:
        raise RuntimeError(f"{arguments!r}: {result.stderr!r}")
    return result


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def pin(root, name):
    return {"path": name, "sha256": hashlib.sha256((root / name).read_bytes()).hexdigest()}


def caller(root, library):
    source = root / "src"
    source.mkdir(parents=True)
    shutil.copyfile(FIXTURE / "caller.rs", source / "main.rs")
    path = json.dumps(str(library))
    (root / "Cargo.toml").write_text(
        '[package]\nname = "station-caller"\nversion = "0.1.0"\nedition = "2024"\n'
        f'[dependencies]\nstation-observer = {{ path = {path} }}\n'
    )
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)
    run("cargo", "+1.97.1", "build", "--offline", "--locked", cwd=root)
    executable = root / "target/debug/station-caller"
    reference = run(executable)
    read, writer = os.pipe()
    os.close(read)
    try:
        try:
            os.write(writer, b"probe")
        except OSError as error:
            assert error.errno == errno.EPIPE
        else:
            raise AssertionError("broken pipe did not report EPIPE")
        failed_sink = run(executable, stderr=writer)
    finally:
        os.close(writer)
    assert failed_sink.stdout == reference.stdout == b"after\n"
    return reference.stderr


with tempfile.TemporaryDirectory(prefix="oddlang-diagnostic-fixture-") as temporary:
    root = Path(temporary)
    source = (FIXTURE / "reference.rs").read_text()
    observations = []
    for name, text in (
        ("original", source),
        ("mutated", source.replace("AtomicBool::new(false)", "AtomicBool::new(true)")
         .replace('"initial\\r\\t"', '"revised"')),
    ):
        reference = root / name / "reference"
        (reference / "src").mkdir(parents=True)
        for file in ("Cargo.toml", "Cargo.lock"):
            shutil.copyfile(FIXTURE / file, reference / file)
        declaration = reference / "src/records.rs"
        declaration.write_text(text)
        (reference / "src/lib.rs").write_text("pub mod records;\n")
        inputs = root / name / "portable"
        inputs.mkdir()
        graph = inputs / "graph.json"
        run(COMPILER, "recover", "structured-diagnostics", declaration, graph)
        run(COMPILER, "recover", "structured-diagnostics", declaration, inputs / "again.json")
        assert graph.read_bytes() == (inputs / "again.json").read_bytes()
        graph_data = json.loads(graph.read_bytes())
        write(inputs / "binding.json", {"sink": "stderr", "borrow": "static"})
        shutil.copyfile(FIXTURE / "Cargo.lock", inputs / "Cargo.lock")
        spec = {"format": "oddlang-native-package-v1", "name": "station-observer",
                "version": "3.4.5", "edition": "2024", "cargoLock": pin(inputs, "Cargo.lock"),
                "dependencies": {"serde_json": {"version": "1.0.154"}},
                "components": [{"kind": "structured-diagnostics", "module": "records",
                                "visibility": "public", "rootReexports": [],
                                "graph": pin(inputs, "graph.json"), "overlay": pin(inputs, "binding.json"),
                                "reexports": ["begin", "observation", "mark", "end"]}]}
        manifest = inputs / "package.json"
        write(manifest, spec)
        candidate = root / name / "candidate"
        run(COMPILER, "build", "native-package", "rust", manifest, candidate, cwd=inputs)
        standalone = root / name / "projection"
        run(COMPILER, "build", "structured-diagnostics", "rust", graph, inputs / "binding.json", standalone)
        member_lock = json.loads((candidate / "package.lock").read_bytes())
        assert hashlib.sha256((standalone / "diagnostics.rs").read_bytes()).hexdigest() == (
            member_lock["components"][0]["sourceSha256"]
        )
        expected = caller(root / name / "reference-caller", reference)
        actual = caller(root / name / "projected-caller", candidate)
        assert actual == expected
        rows = [json.loads(line) for line in actual.splitlines()]
        assert actual == b"".join(json.dumps(row, sort_keys=True, ensure_ascii=False,
                                            separators=(",", ":")).encode() + b"\n" for row in rows)
        assert len(rows) == (4 if name == "original" else 5)
        assert all(row["version"] == "3.4.5" and row["code"] == 7
                   for row in rows if row.get("event") == "startup")
        assert sum(row.get("event") == "startup" for row in rows) == 2
        assert any(row.get("point") == "númeró" and row["reading"] == "control\u0001\n" for row in rows)
        observations.append((graph_data, actual))

        del spec["dependencies"]["serde_json"]
        write(manifest, spec)
        missing_provider = run(COMPILER, "build", "native-package", "rust", manifest, root / "bad-package",
                               check=False)
        assert missing_provider.returncode and b"pinned serde_json dependency" in missing_provider.stderr

    baseline_graph, baseline = observations[0]
    changed_graph, changed = observations[1]
    assert baseline != changed and baseline_graph != changed_graph
    assert b'"point":"before"' not in baseline and b'"point":"before"' in changed
    assert b'"reading":"revised"' in changed

    for index, (invalid, message) in enumerate((
        (source.replace("Ordering::Relaxed", "Ordering::SeqCst", 1), b"unsupported diagnostic statement"),
        (source.replace('"code": 7', '"code": 7, "code": 8'), b"duplicate record field"),
        (source.replace("fn observation(point:", "fn observation(observation:"),
         b"expected read-only text parameter"),
        (source.replace("observation(point:", "observation(null:").replace('"point": point', '"point": null'),
         b"invalid or duplicate parameter null"),
        (source.replace('observation(new, "initial', 'mark(new, "initial'), b"earlier compatible text function"),
    )):
        path = root / f"invalid-{index}.rs"
        path.write_text(invalid)
        result = run(COMPILER, "recover", "structured-diagnostics", path, root / "invalid.json", check=False)
        assert result.returncode and message in result.stderr, result.stderr
    duplicates = root / "duplicate.json"
    duplicates.write_text(json.dumps(baseline_graph).replace(
        '"code": {"kind": "unsigned", "value": 7}',
        '"code": {"kind": "unsigned", "value": 7}, "code": {"kind": "unsigned", "value": 8}',
    ))
    result = run(COMPILER, "build", "structured-diagnostics", "rust", duplicates,
                 inputs / "binding.json", root / "duplicate-graph", check=False)
    assert result.returncode and b"duplicate record field code" in result.stderr
    source_name = baseline_graph["functions"][1]["parameters"][0]["name"]
    baseline_graph["functions"][1]["parameters"][0]["name"] = "null"
    shadow = root / "shadow.json"
    write(shadow, baseline_graph)
    result = run(COMPILER, "build", "structured-diagnostics", "rust", shadow,
                 inputs / "binding.json", root / "shadow-graph", check=False)
    assert result.returncode and b"invalid or duplicate parameter null" in result.stderr
    baseline_graph["functions"][1]["parameters"][0]["name"] = source_name
    baseline_graph["functions"][2]["actions"][0]["target"] = "new"
    bad_graph = root / "bad.json"
    write(bad_graph, baseline_graph)
    result = run(COMPILER, "build", "structured-diagnostics", "rust", bad_graph,
                 inputs / "binding.json", root / "bad-graph", check=False)
    assert result.returncode and b"expected writable text parameter" in result.stderr
    print("Two native source revisions match deterministically, including repeated enable, closed sink and text update.")
