#!/usr/bin/env python3
"""Qualify pinned canonical schemas through native package assembly."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def src(kind, **members):
    return {"kind": kind, **members}


def identity(local):
    return f"urn:weather:{local}"


def field(id, name, shape):
    return {"id": id, "name": name, "type": shape}


def reference(id):
    return src("ref", id=id)


u32 = src("unsigned", bits=32)
envelope, version, sequence, selector, union, variant, endpoint, number, request_seq, error = map(
    identity, ("envelope", "format", "seq", "packet", "direction", "upload", "send", "count", "send-seq", "error"))
payload, label = identity("bytes"), identity("label")
mapped_bytes, mapped_label = identity("native-bytes"), identity("native-label")
binary = src("bounded-base64", policy={
    "maxBytes": 4, "encodedLimitMessage": "encoded weather bytes",
    "decodedLimitMessage": "decoded weather bytes", "syntaxMessage": "weather byte format",
})
text = src("text")
data = {
    "format": "oddlang-json-wire-schema-v1", "id": identity("shape"), "guards": [],
    "types": [
        {"kind": "record", "id": envelope, "fields": [
            field(version, "format", u32), field(sequence, "seq", u32),
            field(selector, "packet", reference(union)),
        ]},
        {"kind": "adjacent", "id": union, "tag": "tag", "content": "data", "variants": [
            {"id": variant, "name": "upload", "payload": src("record", fields=[
                field(number, "count", u32), field(payload, "bytes", binary), field(label, "label", text),
            ])},
        ]},
    ],
}
names = {
    "format": "oddlang-json-wire-schema-rust-v1", "derives": {}, "externals": {},
    "names": dict(zip((envelope, version, sequence, selector, union, variant),
                      ("Frame", "revision", "sequence", "message", "Direction", "Upload"))) | {
                          number: "count", payload: "bytes", label: "label",
                      },
}
native_count = identity("native-count")
revision, limit, invalid, limit_variant, bad_variant = map(
    identity, ("revision", "limit", "invalid", "symbol:limit", "symbol:invalid"))
graph = {
    "format": "oddlang-json-wire-protocol-v1", "id": identity("protocol"), "envelope": envelope,
    "selector": selector, "maximumWireBytes": 115, "sizeError": limit, "admissionError": invalid,
    "errors": {limit: {"variant": limit_variant}, invalid: {"variant": bad_variant}},
    "constants": [{"id": revision, "value": 2, "type": u32}], "literals": {version: revision},
    "endpoints": [{"id": endpoint, "variant": variant, "guards": [], "fields": [
        {"id": request_seq, "from": src("envelope-field", field=sequence)},
        {"id": native_count, "from": src("case-field", field=number)},
        {"id": mapped_bytes, "from": src("case-field", field=payload)},
        {"id": mapped_label, "from": src("case-field", field=label)},
    ]}],
}
target = {
    "format": "oddlang-json-wire-protocol-rust-v1", "errorType": "crate::Failure",
    "errors": {limit_variant: "Limit", bad_variant: "Invalid"},
    "constants": {revision: "FORMAT"}, "fields": {native_count: "count", request_seq: "sequence", mapped_bytes: "bytes", mapped_label: "label"},
    "internalEnvelope": True,
    "endpoints": {endpoint: {"type": "Upload", "encode": "encode_upload", "decode": "decode_upload",
                             "derives": ["Clone", "PartialEq", "Eq"]}},
}
errors = {
    "format": "oddlang-shared-declarations-v1", "id": identity("errors"),
    "errors": [{"id": error, "variants": [
        {"id": limit_variant, "payload": "none", "message": [{"text": "too long"}]},
        {"id": bad_variant, "payload": "none", "message": [{"text": "invalid"}]},
    ]}],
}
error_binding = {
    "format": "oddlang-shared-rust-v1",
    "names": {error: "Failure", limit_variant: "Limit", bad_variant: "Invalid"},
    "variantOrder": {error: [limit_variant, bad_variant]},
}

with tempfile.TemporaryDirectory(prefix="canonical-wire-package-") as directory:
    root = Path(directory)

    def pin(path, data):
        value = json.dumps(data).encode()
        (root / path).write_bytes(value)
        return {"path": path, "sha256": digest(value)}

    lock = shutil.copyfile(HERE / "fixture/Cargo.lock", root / "Cargo.lock")
    shape = {"graph": pin("shape.json", data), "overlay": pin("shape-rust.json", names)}
    protocol = {
        "kind": "wire-protocol", "module": "wire", "visibility": "public", "rootReexports": [],
        "graph": pin("protocol.json", graph), "overlay": pin("protocol-rust.json", target),
        "wireShape": shape, "reexports": ["FORMAT", "Upload", "encode_upload", "decode_upload"],
    }
    manifest = {
        "format": "oddlang-native-package-v1", "name": "radio-frame-consumer", "version": "0.0.0",
        "edition": "2021", "dependencies": {
            "base64": {"version": "0.22.1"}, "serde": {"version": "1.0.229", "features": ["derive"]},
            "serde_json": {"version": "1.0.154"}, "sha2": {"version": "0.10.9"},
        },
        "cargoLock": {"path": "Cargo.lock", "sha256": digest(lock.read_bytes())},
        "components": [{
            "kind": "shared-declarations", "module": "domain", "reexports": ["Failure"],
            "graph": pin("errors.json", errors), "overlay": pin("errors-rust.json", error_binding),
        }, protocol],
    }
    manifest_path = root / "package.json"
    output = root / "out"

    def build(destination, failure=None):
        manifest_path.write_text(json.dumps(manifest))
        result = subprocess.run((CLI, "build", "native-package", "rust", manifest_path, destination),
                                capture_output=True, text=True)
        if failure:
            assert result.returncode and failure in result.stderr and not destination.exists(), result.stderr
        else:
            assert result.returncode == 0, result.stderr

    build(output)
    receipt = json.loads((output / "package.lock").read_bytes())
    assert receipt["components"][1]["wireShape"] == {
        "graphSha256": shape["graph"]["sha256"], "overlaySha256": shape["overlay"]["sha256"],
    }
    alternate = root / "alternate"
    build(alternate)
    assert (alternate / "package.lock").read_bytes() == (output / "package.lock").read_bytes()
    for file, value in receipt["generatedFiles"].items():
        assert digest((output / file).read_bytes()) == value
    binary = root / "probe"
    command = ("cargo", "+1.97.1", "build", "--offline", "--locked", "--quiet")
    env = {**os.environ, "CARGO_ENCODED_RUSTFLAGS": "-Dwarnings"}
    subprocess.run(command, cwd=output, env=env, check=True)
    native = output / "target/debug"
    subprocess.run(("rustc", "+1.97.1", "--edition=2021", "-Dwarnings", HERE / "package_fixture.rs",
                    "-L", f"dependency={native / 'deps'}", "--extern",
                    f"radio_frame_consumer={native / 'libradio_frame_consumer.rlib'}",
                    "-o", binary), check=True)
    subprocess.run((binary,), check=True)
    del protocol["wireShape"]
    build(root / "missing", "wire-protocol component requires wireShape")
    protocol["wireShape"] = shape
    manifest["components"][0]["wireShape"] = shape
    build(root / "unused", "wireShape is only valid")
    del manifest["components"][0]["wireShape"]
    for key in ("graph", "overlay"):
        previous = shape[key]["sha256"]
        shape[key]["sha256"] = "0" * 64
        build(root / f"bad-{key}", "input digest mismatch")
        shape[key]["sha256"] = previous
print("pinned source-free native wire package verified")
