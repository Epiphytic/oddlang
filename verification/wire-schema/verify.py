#!/usr/bin/env python3
"""Recover, project and compile an unrelated JSON wire schema."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parent / "fixture"


def run(*args, failure=None):
    result = subprocess.run((CLI, *args), capture_output=True, text=True)
    if failure is None:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode and failure in result.stderr, result.stderr


def write(path, value):
    path.write_text(json.dumps(value))
    return path


with tempfile.TemporaryDirectory(prefix="wire-schema-test-") as directory:
    root = Path(directory)
    source = FIXTURE / "input.rs"
    selection = {
        "id": "urn:garden:wire",
        "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "types": {"Sensor": "urn:garden:sensor", "Irrigation": "urn:garden:event", "Packet": "urn:garden:packet"},
        "adapters": {
            "signature_text": {"kind": "lower-hex", "bytes": 2, "diagnostic": "signature text"},
            "chunks_text": {"kind": "bounded-base64-list", "policy": {
                "maxItems": 2, "maxItemBytes": 4, "maxTotalBytes": 6, "perItemOverhead": 1,
                "listLimitMessage": "piece list limit", "encodedLimitMessage": "piece text limit",
                "decodedLimitMessage": "piece byte limit", "syntaxMessage": "piece encoding",
            }},
        },
    }
    selected = write(root / "selection.json", selection)
    recovered = root / "observed"
    run("recover", "wire-schema", source, selected, recovered)
    repeat = root / "observed-repeat"
    run("recover", "wire-schema", source, selected, repeat)
    for name in ("graph.json", "rust.json", "observation.json"):
        assert (recovered / name).read_bytes() == (repeat / name).read_bytes()
    observed = json.loads((recovered / "observation.json").read_bytes())
    assert {row["reason"] for row in observed["gaps"]} == {
        "conditional guards require independent evidence",
        "selected adapter policy requires independent witness: chunks_text",
        "selected adapter policy requires independent witness: signature_text",
    }
    assert all(row["kind"] == "behavior" for row in observed["gaps"])
    tampered = root / "tampered.rs"
    tampered.write_bytes(source.read_bytes() + b"\n")
    run("recover", "wire-schema", tampered, selected, root / "bad-source", failure="source digest")
    assert not (root / "bad-source").exists()

    skipped = root / "skipped.rs"
    skipped.write_text(source.read_text().replace("pub meter:", '#[serde(skip_serializing_if = "Option::is_none")] pub meter:'))
    gap = root / "skipped"
    run("recover", "wire-schema", skipped, write(root / "skipped-selection.json",
        dict(selection, sourceSha256=hashlib.sha256(skipped.read_bytes()).hexdigest())), gap)
    gaps = json.loads((gap / "observation.json").read_bytes())["gaps"]
    assert any(row["kind"] == "structure" and "skip_serializing_if" in row["reason"] for row in gaps)
    run("build", "wire-schema", "rust", gap / "graph.json", gap / "rust.json",
        root / "unsafe", failure="unresolved structural evidence")

    lexical_controls = [
        ("macro", source.read_text().replace("pub struct Sensor", "#[serde_with::skip_serializing_none]\npub struct Sensor"),
         "unvetted Rust attribute serde_with::skip_serializing_none"),
        ("item-macro", source.read_text() + "\nmacro_rules! shadow { () => { type u16 = u8; }; } shadow!();\n",
         "unexpanded source item macro shadow"),
        ("verbatim", source.read_text() + "\n#[other::emit] fn incomplete();\n",
         "unparsed source item needs compiler evidence"),
        ("unselected-macro", source.read_text() + "\n#[other::emit] struct Different;\n",
         "source item has unvetted Rust attribute other::emit"),
        ("derive", source.read_text().replace("use serde::{Deserialize, Serialize};",
                                               "use another::{Deserialize, Serialize};"),
         "unverified native derive"),
        ("derive-alias", "use other_derive as serde;\n" + source.read_text(),
         "unverified native derive"),
        ("builtin", source.read_text() + "\ntype Option<T> = std::vec::Vec<T>;\n",
         "unverified generic field type Option"),
    ]
    for name, text, reason in lexical_controls:
        edited = root / (name + ".rs")
        edited.write_text(text)
        candidate = root / name
        run("recover", "wire-schema", edited, write(root / (name + ".json"),
            dict(selection, sourceSha256=hashlib.sha256(edited.read_bytes()).hexdigest())), candidate)
        gaps = json.loads((candidate / "observation.json").read_bytes())["gaps"]
        assert any(row["kind"] == "structure" and reason in row["reason"] for row in gaps)
        run("build", "wire-schema", "rust", candidate / "graph.json", candidate / "rust.json",
            root / (name + "-unsafe"), failure="unresolved structural evidence")

    graph = json.loads((recovered / "graph.json").read_bytes())
    event = next(row for row in graph["types"] if row["id"] == "urn:garden:event")
    adjust = next(row for row in event["variants"] if row["name"] == "adjust_rate")
    fields = {row["name"]: row["id"] for row in adjust["payload"]["fields"]}
    graph["guards"] = [
        {"id": "urn:garden:rule:flow", "owner": event["id"], "variant": adjust["id"],
         "when": {"kind": "bool", "field": fields["enabled"], "value": True},
         "require": {"kind": "unsigned-range", "field": fields["flow"], "minimum": 1, "maximum": 20},
         "error": "urn:garden:flow"},
        {"id": "urn:garden:rule:tag", "owner": event["id"], "variant": adjust["id"],
         "require": {"kind": "unique", "field": fields["tags"]}, "error": "urn:garden:tag"},
    ]
    path = write(root / "reviewed.json", graph)
    generated = root / "native"
    run("build", "wire-schema", "rust", path, recovered / "rust.json", generated)

    reversed_types = dict(graph, types=list(reversed(graph["types"])))
    reordered = root / "same"
    run("build", "wire-schema", "rust", write(root / "same.json", reversed_types),
        recovered / "rust.json", reordered)
    for name in ("wire.rs", "obligations.json", "projection.lock"):
        assert (generated / name).read_bytes() == (reordered / name).read_bytes()
    lock = json.loads((generated / "projection.lock").read_bytes())
    assert lock["obligationsSha256"] == hashlib.sha256((generated / "obligations.json").read_bytes()).hexdigest()
    binding = json.loads((recovered / "rust.json").read_bytes())
    binding["derives"]["urn:garden:sensor"] = ["Clone", "Copy"]
    run("build", "wire-schema", "rust", path, write(root / "bad-derive.json", binding),
        root / "bad-derive", failure="referenced field does not support derive Eq")
    binding = json.loads((recovered / "rust.json").read_bytes())
    binding["derives"]["urn:garden:event"].append("Copy")
    run("build", "wire-schema", "rust", path, write(root / "bad-copy.json", binding),
        root / "bad-copy", failure="referenced field does not support derive Copy")

    consumer = root / "consumer"
    shutil.copytree(FIXTURE, consumer)
    shutil.copyfile(generated / "wire.rs", consumer / "src/projected.rs")
    singleton = json.loads(path.read_bytes())
    single_event = next(row for row in singleton["types"] if row["id"] == event["id"])
    single_event["variants"] = [adjust]
    singleton_binding = json.loads((recovered / "rust.json").read_bytes())
    for variant in event["variants"]:
        if variant["id"] != adjust["id"]:
            del singleton_binding["names"][variant["id"]]
    single_output = root / "single"
    run("build", "wire-schema", "rust", write(root / "single.json", singleton),
        write(root / "single-rust.json", singleton_binding), single_output)
    shutil.copyfile(single_output / "wire.rs", consumer / "src/single.rs")
    subprocess.run(("cargo", "+1.97.1", "run", "--locked", "--offline", "--quiet"), cwd=consumer,
                   check=True, env={**os.environ, "CARGO_ENCODED_RUSTFLAGS": "-Dwarnings"})

    graph["guards"].reverse()
    mutant = root / "mutant"
    run("build", "wire-schema", "rust", write(root / "changed.json", graph),
        recovered / "rust.json", mutant)
    shutil.copyfile(mutant / "wire.rs", consumer / "src/projected.rs")
    result = subprocess.run(("cargo", "+1.97.1", "run", "--locked", "--offline", "--quiet"),
                            cwd=consumer, capture_output=True, text=True)
    assert result.returncode and 'Err("urn:garden:flow")' in result.stderr, result.stderr

    graph["guards"][0]["require"]["field"] = fields["flow"]
    run("build", "wire-schema", "rust", write(root / "bad-guard.json", graph),
        recovered / "rust.json", root / "bad-guard", failure="predicate field type")
print("structural recovery, ordered guards, and unrelated wire projection passed")
