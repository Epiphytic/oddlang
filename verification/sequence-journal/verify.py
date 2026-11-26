#!/usr/bin/env python3
"""Qualify a bounded telemetry queue independently of a production journal."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

BINARY = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parent / "fixture"
PREFIX = "urn:telemetry-queue:"


def run(*args, cwd=None, error=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if error is None:
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout.strip()
    assert result.returncode != 0 and error in result.stderr, result.stdout + result.stderr


def write(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pin(path):
    return {"path": path.name, "sha256": digest(path)}


with tempfile.TemporaryDirectory(prefix="sequence-journal-") as directory:
    work = Path(directory)
    graph = FIXTURE / "graph.json"
    overlay = FIXTURE / "rust.json"
    raw = json.loads(graph.read_bytes())
    binding = json.loads(overlay.read_bytes())
    graph_pin = run(BINARY, "sequence-journal", "digest", graph)
    overlay_pin = run(BINARY, "sequence-journal", "overlay-digest", overlay)

    def generate(graph, overlay, output):
        graph_digest = run(BINARY, "sequence-journal", "digest", graph)
        binding_digest = run(BINARY, "sequence-journal", "overlay-digest", overlay)
        run(BINARY, "build", "sequence-journal", "rust", graph, overlay, output,
            "--graph-sha256", graph_digest, "--overlay-sha256", binding_digest)
        lock = json.loads((output / "projection.lock").read_bytes())
        assert graph_digest == lock["graphSha256"] and binding_digest == lock["overlaySha256"]
        assert digest(output / "sequence_journal.rs") == lock["sourceSha256"]
        assert digest(output / "obligations.json") == lock["obligationsSha256"]
        return output

    positive = generate(graph, overlay, work / "positive")
    obligations = json.loads((positive / "obligations.json").read_bytes())
    assert set(obligations["operations"]) == {op["id"] for op in raw["operations"]}
    for op in raw["operations"]:
        assert obligations["operations"][op["id"]]["orderedCases"] == [row["id"] for row in op["cases"]]
    assert set(obligations["cases"]) == {c["id"] for op in raw["operations"] for c in op["cases"]}
    assert obligations["cases"][PREFIX + "probe_evict_rollback:0"]["effectFailures"] == [
        {"effect": "evict-receipt", "error": PREFIX + "Bounds", "state": "unchanged"},
        {"effect": "append", "error": PREFIX + "Bounds", "state": "unchanged"},
    ]

    equivalent = copy.deepcopy(raw)
    equivalent["errors"].reverse()
    equivalent["operations"].reverse()
    equivalent["limits"]["max-cursor"]["cursor"] = 7
    equivalent["states"][0]["initialCursor"] = 0
    replay = generate(write(work / "equivalent.json", equivalent), overlay, work / "replay")
    for filename in ("sequence_journal.rs", "projection.lock", "obligations.json"):
        assert (positive / filename).read_bytes() == (replay / filename).read_bytes()

    changed = copy.deepcopy(raw)
    post = next(op for op in changed["operations"] if op["id"] == PREFIX + "post")
    post["cases"][1], post["cases"][5] = post["cases"][5], post["cases"][1]
    reordered = generate(write(work / "changed-order.json", changed), overlay, work / "changed-order")
    changed = copy.deepcopy(raw)
    changed["limits"]["max-records"]["size"] = 3
    larger = generate(write(work / "larger.json", changed), overlay, work / "larger")

    def consumer(projection, name, error=None):
        crate = work / name
        shutil.copytree(FIXTURE, crate)
        shutil.copyfile(projection / "sequence_journal.rs", crate / "src/journal.rs")
        run("cargo", "+1.97.1", "run", "--locked", "--offline", "--release", "--quiet",
            "--target-dir", work / "target", cwd=crate, error=error)

    consumer(positive, "baseline")
    consumer(reordered, "ordering-control", "left: Err(Gap)")
    consumer(larger, "capacity-control", "left: 1")

    flat_binding = copy.deepcopy(binding)
    flat_binding["storage"] = json.loads((FIXTURE / "storage.json").read_bytes())
    flat_overlay = write(work / "flat-overlay.json", flat_binding)
    flat = generate(graph, flat_overlay, work / "flat-generated")
    consumer(flat, "flat-public")
    mixed_binding = copy.deepcopy(flat_binding)
    del mixed_binding["storage"][PREFIX + "near-cursor-limit"]
    mixed = generate(graph, write(work / "mixed-overlay.json", mixed_binding), work / "mixed-generated")
    consumer(mixed, "mixed-public")
    empty_storage = write(work / "empty-storage.json", {**binding, "storage": {}})
    assert run(BINARY, "sequence-journal", "overlay-digest", empty_storage) == overlay_pin
    flat_crate = work / "flat-public"
    journal = flat_crate / "src/journal.rs"
    with journal.open("ab") as source:
        source.write(b"\n")
        source.write((FIXTURE / "lexical.rs").read_bytes())
    actual = run("cargo", "+1.97.1", "test", "--locked", "--offline", "--release", "--quiet",
                 "--target-dir", work / "target", cwd=flat_crate)
    assert "2 passed; 0 failed" in actual
    private = flat_crate / "src/main.rs"
    with private.open("a") as source:
        source.write("""
#[allow(dead_code)]
fn read_private(queue: &journal::TelemetryQueue) -> usize {
    queue.confirmations.len()
}
""")
    run("cargo", "+1.97.1", "check", "--locked", "--offline", "--quiet",
        "--target-dir", work / "target", cwd=flat_crate, error="field `confirmations`")
    invalid_storage = copy.deepcopy(flat_binding)
    invalid_storage["storage"][PREFIX + "state"]["end"] = "consumed"
    run(BINARY, "sequence-journal", "overlay-digest",
        write(work / "duplicate-storage.json", invalid_storage), error="duplicate private journal")
    invalid_storage = copy.deepcopy(flat_binding)
    invalid_storage["storage"][PREFIX + "unknown"] = invalid_storage["storage"][PREFIX + "state"]
    overlay_with_unknown = write(work / "unknown-storage.json", invalid_storage)
    unknown_pin = run(BINARY, "sequence-journal", "overlay-digest", overlay_with_unknown)
    run(BINARY, "build", "sequence-journal", "rust", graph, overlay_with_unknown,
        work / "unknown-storage", "--graph-sha256", graph_pin,
        "--overlay-sha256", unknown_pin, error="unknown journal state")

    for name, definitions, expected in (
        ("cyclic", {"a": {"expr": "b"}, "b": {"expr": "a"}}, "named expression cycle"),
        ("missing", {"unused": {"expr": "missing"}}, "unknown named expression"),
        ("bad-use", {"usable": {"arg": "unknown"}}, "unknown argument"),
    ):
        invalid = copy.deepcopy(raw)
        invalid["expressions"].update(definitions)
        run(BINARY, "sequence-journal", "digest", write(work / f"{name}.json", invalid),
            error=expected)
    invalid = copy.deepcopy(raw)
    op = next(op for op in invalid["operations"] if op["id"] == PREFIX + "view")
    op["cases"][0]["when"] = [{"cursor": 0}]
    run(BINARY, "sequence-journal", "digest", write(work / "invalid-type.json", invalid),
        error="expected Flag, found Cursor")
    invalid = copy.deepcopy(raw)
    op = next(op for op in invalid["operations"] if op["id"] == PREFIX + "ack")
    op["access"] = "read"
    run(BINARY, "sequence-journal", "digest", write(work / "invalid-write.json", invalid),
        error="only fallible write")
    invalid = copy.deepcopy(raw)
    op = next(op for op in invalid["operations"] if op["id"] == PREFIX + "view")
    op["cases"].pop()
    run(BINARY, "sequence-journal", "digest", write(work / "missing-final.json", invalid),
        error="exactly the final case")
    for document, field, verb in ((raw, "limits", "digest"),
                                  (raw, "expressions", "digest"),
                                  (binding, "methods", "overlay-digest")):
        path = work / f"duplicate-{field}.json"
        marker = f'"{field}": {{'
        key, value = next(iter(document[field].items()))
        duplicate = json.dumps(key) + ": " + json.dumps(value) + ", "
        path.write_text(json.dumps(document).replace(marker, marker + duplicate, 1),
                        encoding="utf-8")
        run(BINARY, "sequence-journal", verb, path, error="duplicate binding key")

    invalid_overlay = copy.deepcopy(binding)
    invalid_overlay["methods"].pop(PREFIX + "view")
    incomplete = write(work / "incomplete.json", invalid_overlay)
    invalid_pin = run(BINARY, "sequence-journal", "overlay-digest", incomplete)
    run(BINARY, "build", "sequence-journal", "rust", graph, incomplete, work / "bad-overlay",
        "--graph-sha256", graph_pin, "--overlay-sha256", invalid_pin,
        error="binding must cover")
    run(BINARY, "build", "sequence-journal", "rust", graph, overlay, work / "bad-digest",
        "--graph-sha256", "0" * 64, "--overlay-sha256", overlay_pin, error="graph digest mismatch")

    shadowed = copy.deepcopy(binding)
    shadowed["types"][PREFIX + "state"] = "Result"
    shadowed = write(work / "shadowed-rust.json", shadowed)
    shadow_pin = run(BINARY, "sequence-journal", "overlay-digest", shadowed)
    run(BINARY, "build", "sequence-journal", "rust", graph, shadowed, work / "shadowed",
        "--graph-sha256", graph_pin, "--overlay-sha256", shadow_pin,
        error="reserved journal Rust type")

    # Assemble the journal with a separately projected shared error type.
    inputs = work / "package-input"
    inputs.mkdir()
    lock = inputs / "Cargo.lock"
    shutil.copyfile(FIXTURE / "Cargo.lock", lock)
    journal_graph = inputs / "journal.json"
    journal_overlay = inputs / "journal-rust.json"
    shutil.copyfile(graph, journal_graph)
    shutil.copyfile(flat_overlay, journal_overlay)
    error_id = PREFIX + "shared-error"
    shared_graph = {
        "format": "oddlang-shared-declarations-v1", "id": PREFIX + "declarations",
        "errors": [{"id": error_id, "variants": [
            {"id": error, "payload": "none", "message": [{"text": error.removeprefix(PREFIX)}]}
            for error in raw["errors"]
        ]}],
    }
    shared_overlay = {
        "format": "oddlang-shared-rust-v1",
        "names": {error_id: "QueueError", **binding["errorVariants"]},
        "constantTypes": {}, "variantOrder": {error_id: raw["errors"]},
    }
    package = {
        "format": "oddlang-native-package-v1", "name": "telemetry-journal-fixture",
        "version": "0.1.0", "edition": "2021", "dependencies": {"sha2": {"version": "0.10.9"}},
        "cargoLock": pin(lock), "components": [
            {"kind": "shared-declarations", "module": "queue", "visibility": "public",
             "graph": pin(write(inputs / "shared.json", shared_graph)),
             "overlay": pin(write(inputs / "shared-rust.json", shared_overlay)),
             "reexports": ["QueueError"], "rootReexports": ["QueueError"]},
            {"kind": "sequence-journal", "module": "queue", "visibility": "public",
             "graph": pin(journal_graph), "overlay": pin(journal_overlay),
             "reexports": ["BoundaryQueue", "TelemetryQueue"], "rootReexports": []},
        ],
    }
    assembled = work / "assembled"
    run(BINARY, "build", "native-package", "rust", write(inputs / "package.json", package), assembled)
    package_lock = json.loads((assembled / "package.lock").read_bytes())
    assert package_lock["components"][1]["projection"]["obligationsSha256"] == digest(
        positive / "obligations.json"
    )
    journal_leaf = assembled / "src/group_0/__projection_1.rs"
    with journal_leaf.open("ab") as source:
        source.write(b"\n")
        source.write((FIXTURE / "lexical.rs").read_bytes())
    (assembled / "tests").mkdir()
    caller = (FIXTURE / "src/main.rs").read_text()
    caller = caller[caller.index("fn main() {"):].replace("fn main()", "#[test]\nfn queue()")
    caller = caller.replace("use journal::{BoundaryQueue, TelemetryQueue};",
                            "use telemetry_journal_fixture::queue::{BoundaryQueue, TelemetryQueue};")
    (assembled / "tests/queue.rs").write_text("use telemetry_journal_fixture::QueueError;\n" + caller)
    result = run("cargo", "+1.97.1", "test", "--locked", "--offline", "--release", "--quiet",
                 "--target-dir", work / "target", cwd=assembled)
    assert "1 passed; 0 failed" in result and "2 passed; 0 failed" in result
print("sequence journal passed 256 native boundary cases, atomic rollback, mutations and package composition")
