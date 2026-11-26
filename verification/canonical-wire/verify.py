#!/usr/bin/env python3
"""Compile unrelated checked native operations and their canonical JSON framing."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
ROOT = Path(__file__).resolve().parent


def write(path, value):
    path.write_text(json.dumps(value))
    return path


def run(*args, failure=None):
    result = subprocess.run((CLI, *args), capture_output=True, text=True)
    if failure:
        assert result.returncode and failure in result.stderr, result.stderr
    else:
        assert result.returncode == 0, result.stderr


def field(owner, name, shape):
    return {"id": f"urn:radio:{owner}:{name}", "name": name, "type": shape}


text = {"kind": "text"}
u32 = {"kind": "unsigned", "bits": 32}
u64 = {"kind": "unsigned", "bits": 64}
boolean = {"kind": "bool"}
env = "urn:radio:envelope"
message = "urn:radio:message"
send = "urn:radio:send"
read = "urn:radio:read"
radio = "urn:radio:protocol"
revision = "urn:radio:version"
shape = {
    "format": "oddlang-json-wire-schema-v1", "id": "urn:radio:wire", "guards": [],
    "types": [
        {"kind": "record", "id": env, "fields": [
            field("envelope", "protocol", text), field("envelope", "version", u32),
            field("envelope", "sequence", u64), field("envelope", "message", {"kind": "ref", "id": message}),
        ]},
        {"kind": "adjacent", "id": message, "tag": "dir", "content": "payload", "variants": [
            {"id": send, "name": "send", "payload": {"kind": "record", "fields": [field("send", "gain", u32)]}},
            {"id": read, "name": "read", "payload": {"kind": "newtype", "type": boolean}},
        ]},
    ],
}
binding = {
    "format": "oddlang-json-wire-schema-rust-v1",
    "names": {env: "Envelope", message: "Message", send: "Send", read: "Read",
              **{f"urn:radio:envelope:{f}": f for f in ("protocol", "version", "sequence", "message")},
              "urn:radio:send:gain": "gain"},
    "derives": {}, "externals": {},
}
protocol = {
    "format": "oddlang-json-wire-protocol-v1", "id": "urn:radio:frame",
    "envelope": env, "selector": "urn:radio:envelope:message", "maximumWireBytes": 120,
    "sizeError": "urn:radio:size", "admissionError": "urn:radio:bad",
    "constants": [
        {"id": radio, "value": "radio", "type": text},
        {"id": revision, "value": 7, "type": u32},
    ],
    "literals": {"urn:radio:envelope:protocol": radio, "urn:radio:envelope:version": revision},
    "endpoints": [
        {"id": "urn:radio:tx", "variant": send, "fields": [
            {"id": "urn:radio:tx:seq", "from": {"kind": "envelope-field", "field": "urn:radio:envelope:sequence"}},
            {"id": "urn:radio:tx:level", "from": {"kind": "case-field", "field": "urn:radio:send:gain"}},
        ], "guards": [
            {"id": "urn:radio:gain-check", "require": {
                "kind": "unsigned-range", "field": "urn:radio:tx:level", "minimum": 1, "maximum": 20},
             "error": "urn:radio:range"},
        ]},
        {"id": "urn:radio:rx", "variant": read, "fields": [
            {"id": "urn:radio:rx:seq", "from": {"kind": "envelope-field", "field": "urn:radio:envelope:sequence"}},
            {"id": "urn:radio:rx:bit", "from": {"kind": "case-value"}},
        ], "guards": []},
    ],
}
target = {
    "format": "oddlang-json-wire-protocol-rust-v1",
    "errorType": "crate::Failure",
    "errors": {"urn:radio:size": {"variant": "Oversize"},
               "urn:radio:bad": {"variant": "Malformed", "argument": "json-wire"},
               "urn:radio:range": {"variant": "Range"}},
    "constants": {radio: "PROTOCOL", revision: "FORMAT"},
    "fields": {"urn:radio:tx:seq": "sequence", "urn:radio:tx:level": "gain",
               "urn:radio:rx:seq": "sequence", "urn:radio:rx:bit": "reading"},
    "endpoints": {
        "urn:radio:tx": {"type": "Transmit", "encode": "encode_message", "decode": "decode_message",
                        "derives": ["Clone", "PartialEq", "Eq"]},
        "urn:radio:rx": {"type": "Readback", "encode": "encode_reply", "decode": "decode_reply",
                        "derives": ["Clone", "PartialEq", "Eq"]},
    },
}

def operand(scope, *fields):
    return {"scope": scope, "fields": list(fields)}


def compare(left, right):
    return {"kind": "compare", "left": {"kind": "field", "value": left},
            "operator": "at-most", "right": {"kind": "field", "value": right}}


tx, rx = protocol["endpoints"]
tx["timed"] = [{"kind": "require", "id": "urn:radio:clock",
                "predicate": compare(operand("value", "urn:radio:tx:seq"), operand("clock")),
                "error": "urn:radio:early"}]
target["endpoints"]["urn:radio:tx"]["timed"] = "validate_at"
target["errors"]["urn:radio:early"] = {"variant": "Early"}
protocol["digests"] = [
    {"id": "urn:radio:raw-digest", "kind": "bytes"},
    {"id": "urn:radio:tx-digest", "kind": "case", "union": message, "variant": send,
     "parameters": [{"id": "urn:radio:level-arg", "field": "urn:radio:send:gain", "passing": "owned"}],
     "steps": []},
]
target["auxiliary"] = {"urn:radio:raw-digest": "raw_digest", "urn:radio:tx-digest": "transmit_digest",
                       "urn:radio:level-arg": "gain"}

with tempfile.TemporaryDirectory(prefix="canonical-wire-") as directory:
    temp = Path(directory)
    shape_path = write(temp / "shape.json", shape)
    shape_binding = write(temp / "shape-rust.json", binding)
    graph = write(temp / "protocol.json", protocol)
    overlay = write(temp / "rust.json", target)
    output = temp / "output"
    common = ("build", "wire-protocol", "rust", shape_path, shape_binding)
    run(*common, graph, overlay, output)
    repeat = temp / "repeat"
    run(*common, graph, overlay, repeat)
    for file in ("wire.rs", "obligations.json", "projection.lock"):
        assert (output / file).read_bytes() == (repeat / file).read_bytes()
    lock = json.loads((output / "projection.lock").read_bytes())
    for file, key in (("wire.rs", "wireSha256"), ("obligations.json", "obligationsSha256")):
        assert lock[key] == hashlib.sha256((output / file).read_bytes()).hexdigest()
    bad = json.loads(graph.read_bytes())
    bad["endpoints"][0]["fields"].pop()
    run(*common, write(temp / "missing.json", bad), overlay, temp / "missing", failure="endpoint must map")
    bad = json.loads(graph.read_bytes())
    bad["endpoints"][0]["steps"] = bad["endpoints"][0].pop("timed")
    run(*common, write(temp / "clock.json", bad), overlay, temp / "clock",
        failure="timed guards require")
    consumer = temp / "consumer"
    shutil.copytree(ROOT / "fixture", consumer)
    shutil.copyfile(output / "wire.rs", consumer / "src/generated.rs")
    subprocess.run(("cargo", "+1.97.1", "run", "--locked", "--offline", "--quiet"), cwd=consumer,
                   env={**os.environ, "CARGO_ENCODED_RUSTFLAGS": "-Dwarnings"}, check=True)
print("canonical source-free native wire operations passed")
