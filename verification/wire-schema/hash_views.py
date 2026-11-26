"""Qualify typed hash views with an unrelated native consumer and negative controls."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

cli = Path(sys.argv[1]).resolve()
owner, tag, seq, payload = ("urn:sensor:" + key for key in ("sample", "state", "seq", "payload"))
on, off, digest, label = ("urn:sensor:" + key for key in ("on", "off", "digest", "label"))
error = "urn:sensor:bounds"
graph = {
    "format": "oddlang-json-wire-schema-v1", "id": "urn:sensor:schema",
    "types": [
        {"kind": "record", "id": owner, "fields": [
            {"id": seq, "name": "sequence", "type": {"kind": "unsigned", "bits": 16}},
            {"id": payload, "name": "payload", "type": {"kind": "bounded-base64", "policy": {
                "maxBytes": 8, "encodedLimitMessage": "too long",
                "decodedLimitMessage": "decoded length", "syntaxMessage": "wrong encoding"}}},
        ]},
        {"kind": "external", "id": tag},
    ],
    "guards": [{"id": "urn:sensor:payload-length", "owner": owner, "error": error,
                "require": {"kind": "count", "field": payload, "minimum": 1, "maximum": 8}}],
    "hashTags": {tag: {on: 42, off: 17}},
    "hashViews": [
        {"id": digest, "owner": owner, "algorithm": "sha256", "domain": [0, 255],
         "validate": True, "parameters": [{"id": "urn:sensor:parameter", "external": tag}],
         "fields": [{"field": seq, "encoding": "unsigned-be"},
                    {"field": payload, "encoding": "u32-be-length-prefixed-bytes"},
                    {"parameter": "urn:sensor:parameter", "encoding": "external-tag-u8"}],
         "output": {"kind": "bytes"}},
        {"id": label, "owner": owner, "algorithm": "sha256", "domain": [71],
         "fields": [{"field": seq, "encoding": "unsigned-be"}],
         "output": {"kind": "lower-hex", "prefix": "sensor-"}},
    ],
}
binding = {
    "format": "oddlang-json-wire-schema-rust-v1",
    "names": {owner: "Sample", seq: "seq", payload: "payload"},
    "derives": {}, "externals": {tag: "crate::State"},
    "hashVariants": {on: "On", off: "Off"},
    "hashViews": {
        digest: {"method": "checksum", "library": "sha2", "parameters": {"urn:sensor:parameter": "state"}},
        label: {"method": "label", "library": "sha2"},
    },
}


def run(*args, cwd=None, error=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if error:
        assert result.returncode and error in result.stderr, result
    else:
        assert not result.returncode, result


with tempfile.TemporaryDirectory() as path:
    root = Path(path)

    def project(graph, binding, out, error=None):
        src, overlay = root / "graph.json", root / "binding.json"
        src.write_text(json.dumps(graph))
        overlay.write_text(json.dumps(binding))
        run(cli, "build", "wire-schema", "rust", src, overlay, out, error=error)

    out = root / "src"
    project(graph, binding, out)
    obligations = json.loads((out / "obligations.json").read_bytes())["obligations"]
    for view in graph["hashViews"]:
        assert any(row["declaration"] == view["id"] and row["role"] == "typed-hash-view"
                   for row in obligations)
    (root / "Cargo.toml").write_text("""[package]
name = "hash-view-check"
version = "0.0.0"
edition = "2024"

[dependencies]
sha2 = "=0.10.9"
serde = { version = "=1.0.229", features = ["derive"] }
base64 = "=0.22.1"
""")
    (out / "main.rs").write_text(f"""#![deny(warnings)]
mod wire;
#[derive(serde::Serialize, serde::Deserialize)]
pub enum State {{ On, Off }}
fn main() {{
    let mut sample = wire::Sample {{ seq: 0x0102, payload: vec![0xfa, 0xf0] }};
    assert_eq!(sample.checksum(State::On), Ok({list(hashlib.sha256(bytes.fromhex("00ff010200000002faf02a")).digest())!r}));
    assert_eq!(sample.checksum(State::Off), Ok({list(hashlib.sha256(bytes.fromhex("00ff010200000002faf011")).digest())!r}));
    sample.payload.clear();
    assert_eq!(sample.checksum(State::On), Err("{error}"));
    assert_eq!(sample.label(), "sensor-{hashlib.sha256(bytes.fromhex('470102')).hexdigest()}");
}}
""")
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)
    run("cargo", "+1.97.1", "run", "--locked", "--offline", cwd=root)
    reversed_graph = copy.deepcopy(graph)
    reversed_graph["hashViews"].reverse()
    reversed_graph["types"].reverse()
    again = root / "again"
    project(reversed_graph, binding, again)
    for artifact in ("wire.rs", "obligations.json", "projection.lock"):
        assert (out / artifact).read_bytes() == (again / artifact).read_bytes()

    for change, diagnostic in [
        (lambda g, b: g["hashViews"][0]["fields"][0].update(encoding="bool-u8"), "hash slot type"),
        (lambda g, b: g["hashViews"][0].update(validate=False), "unconditional count guard"),
        (lambda g, b: g["hashTags"][tag].update({off: 42}), "hash tag table"),
        (lambda g, b: b["hashViews"][digest].update(method="validate_wire"), "hash method binding"),
        (lambda g, b: b["hashViews"][digest]["parameters"].update({"urn:sensor:parameter": "__oddlang_hash"}), "hash parameter binding"),
    ]:
        g, b = copy.deepcopy(graph), copy.deepcopy(binding)
        change(g, b)
        bad = root / "bad"
        project(g, b, bad, error=diagnostic)
        assert not bad.exists()

print("Typed hash views match independent bytes, compile without warnings, and reject invalid bindings.")
