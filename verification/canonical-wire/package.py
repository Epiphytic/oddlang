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
payload, label, samples = map(identity, ("bytes", "label", "samples"))
mapped_bytes, mapped_label, mapped_samples = map(identity, ("native-bytes", "native-label", "native-samples"))
binary = src("bounded-base64", policy={
    "maxBytes": 4, "encodedLimitMessage": "encoded weather bytes",
    "decodedLimitMessage": "decoded weather bytes", "syntaxMessage": "weather byte format",
})
text = src("text")
sample_record, sample_tag = map(identity, ("sample", "sample-tag"))
data = {
    "format": "oddlang-json-wire-schema-v1", "id": identity("shape"), "guards": [],
    "types": [
        {"kind": "record", "id": sample_record, "fields": [field(sample_tag, "tag", text)]},
        {"kind": "record", "id": envelope, "fields": [
            field(version, "format", u32), field(sequence, "seq", u32),
            field(selector, "packet", reference(union)),
        ]},
        {"kind": "adjacent", "id": union, "tag": "tag", "content": "data", "variants": [
            {"id": variant, "name": "upload", "payload": src("record", fields=[
                field(number, "count", u32), field(payload, "bytes", binary), field(label, "label", text),
                field(samples, "samples", src("list", item=src("list", item=reference(sample_record)))),
            ])},
        ]},
    ],
}
names = {
    "format": "oddlang-json-wire-schema-rust-v1",
    "derives": {sample_record: ["Clone", "PartialEq", "Eq"]}, "externals": {},
    "names": dict(zip((envelope, version, sequence, selector, union, variant),
                      ("Frame", "revision", "sequence", "message", "Direction", "Upload"))) | {
                          number: "count", payload: "bytes", label: "label", samples: "samples",
                          sample_record: "Sample", sample_tag: "tag",
                      },
}
native_count = identity("native-count")
revision, limit, invalid, limit_variant, bad_variant = map(
    identity, ("revision", "limit", "invalid", "symbol:limit", "symbol:invalid"))
graph = {
    "format": "oddlang-json-wire-protocol-v1", "id": identity("protocol"), "envelope": envelope,
    "selector": selector, "maximumWireBytes": 160, "sizeError": limit, "admissionError": invalid,
    "errors": {limit: {"variant": limit_variant}, invalid: {"variant": bad_variant}},
    "constants": [{"id": revision, "value": 2, "type": u32}], "literals": {version: revision},
    "endpoints": [{"id": endpoint, "variant": variant, "guards": [], "fields": [
        {"id": request_seq, "from": src("envelope-field", field=sequence)},
        {"id": native_count, "from": src("case-field", field=number)},
        {"id": mapped_bytes, "from": src("case-field", field=payload)},
        {"id": mapped_label, "from": src("case-field", field=label)},
        {"id": mapped_samples, "from": src("case-field", field=samples)},
    ]}],
}
def operand(scope, *fields):
    return {"scope": scope, "fields": list(fields)}


def require(name, predicate, error=invalid):
    return src("require", id=identity(name), predicate=predicate, error=error)


def compare(left, operator, value):
    return src("compare", left=left, operator=operator, right=src("constant", value=value))


def check(name, value, target):
    return src("check-record", id=identity(name), value=value, check=target)


def sample(name, **kwargs):
    return src(name, value=operand("value", sample_tag), **kwargs)


sample_check, name_check, empty_check, unused_check = map(
    identity, ("check:sample", "check:text", "check:empty", "check:unused"))
record_checks = {
    sample_check: {"owner": sample_record, "steps": [
        check("check-name", operand("value"), name_check),
        require("sample-json-size", compare(sample("json-string-length"), "at-most", 16), error=limit),
        check("allow-empty-rule", operand("value"), empty_check),
    ]},
    name_check: {"owner": sample_record, "steps": [
        require("sample-name", src("any", items=[
            sample("text-equal", text="hail"), sample("text-prefix", text="rain:"),
        ])),
        require("sample-nul", sample("text-excludes-nul")),
    ]},
    empty_check: {"owner": sample_record, "steps": []},
    unused_check: {"owner": sample_record, "steps": [
        require("unused-tag-rule", sample("text-equal", text="unused rule sentinel")),
    ]},
}
graph["recordChecks"] = record_checks
byte_length = src("length", value=operand("value", mapped_bytes))
steps = [
    require("byte-length", src("all", items=[
        compare(byte_length, "at-least", 1), compare(byte_length, "at-most", 4),
    ])),
    src("for-each", id=identity("each-batch"), value=operand("value", mapped_samples), bind="batch", steps=[
        src("for-each", id=identity("each-sample"), value=operand("batch"), bind="sample", steps=[
            check("apply-sample-check", operand("sample"), sample_check),
        ]),
    ]),
]
graph["endpoints"][0]["steps"] = steps
target = {
    "format": "oddlang-json-wire-protocol-rust-v1", "errorType": "crate::Failure",
    "errors": {limit_variant: "Limit", bad_variant: "Invalid"},
    "constants": {revision: "FORMAT"}, "fields": {native_count: "count", request_seq: "sequence", mapped_bytes: "bytes", mapped_label: "label", mapped_samples: "samples"},
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
        "wireShape": shape, "reexports": ["FORMAT", "Sample", "Upload", "encode_upload", "decode_upload"],
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
    def project(destination):
        subprocess.run(
            (CLI, "build", "wire-protocol", "rust", root / "shape.json", root / "shape-rust.json",
             root / "protocol.json", root / "protocol-rust.json", destination),
            check=True, capture_output=True,
        )

    projection = root / "wire"
    project(projection)
    wire = (projection / "wire.rs").read_bytes()
    assert b"unused rule sentinel" not in wire
    obligations = (projection / "obligations.json").read_bytes()
    rows = json.loads(obligations)["obligations"]
    assert {row["declaration"]: row["owner"] for row in rows if row["role"] == "record-check"} == {
        key: sample_record for key in record_checks
    }
    assert any(row.get("recordCheck") == sample_check and row["step"].get("check") == name_check
               for row in rows)
    assert any(row.get("endpoint") == endpoint and row.get("step", {}).get("check") == sample_check
               for row in rows)
    graph["recordChecks"] = dict(reversed(list(record_checks.items())))
    pin("protocol.json", graph)
    reordered = root / "reordered"
    project(reordered)
    assert (reordered / "wire.rs").read_bytes() == wire
    assert (reordered / "obligations.json").read_bytes() == obligations
    graph["recordChecks"] = record_checks

    traversal = steps[1]
    nested = traversal["steps"][0]
    apply_check = nested["steps"][0]
    check_name = record_checks[name_check]["steps"][0]
    check_size = record_checks[sample_check]["steps"][1]
    mutations = (
        (traversal, "value", operand("value", mapped_label), "guard iteration requires a typed list"),
        (nested, "bind", "batch", "empty or shadowed guard scope name"),
        (nested, "id", apply_check["id"], "duplicate wire declaration"),
        (check_name["predicate"]["items"][0], "value", operand("value"), "guard expects text"),
        (check_size["predicate"]["left"], "value", operand("value"), "guard expects text"),
        (apply_check, "check", identity("missing-check"), "undeclared record check"),
        (apply_check, "value", operand("batch"), "record check requires its declared type"),
        (record_checks[sample_check], "owner", envelope, "record check requires its declared type"),
        (record_checks[unused_check], "owner", union, "a shared check requires a named record"),
        (record_checks[unused_check], "owner", identity("unknown-record"), "unknown guard type"),
        (record_checks[unused_check], "steps",
         [check("recurse", operand("value"), unused_check)], "recursive shared record checks"),
        (record_checks[empty_check], "steps",
         [check("cycle", operand("value"), sample_check)], "recursive shared record checks"),
        (graph, "recordChecks", {**record_checks, endpoint: {"owner": sample_record, "steps": []}},
         "duplicate wire declaration"),
        (target["endpoints"][endpoint], "encode", "__oddlang_record_check_0",
         "invalid native endpoint API name"),
    )
    for index, (node, key, value, message) in enumerate(mutations):
        previous = node[key]
        node[key] = value
        protocol["graph"] = pin("protocol.json", graph)
        protocol["overlay"] = pin("protocol-rust.json", target)
        build(root / f"invalid-guard-{index}", message)
        node[key] = previous
    duplicate_key = json.dumps(graph).replace(
        json.dumps(empty_check) + ": {",
        json.dumps(empty_check) + ': {"owner": ' + json.dumps(sample_record) + ', "steps": []}, '
        + json.dumps(empty_check) + ": {",
        1,
    ).encode()
    (root / "protocol.json").write_bytes(duplicate_key)
    protocol["graph"] = {"path": "protocol.json", "sha256": digest(duplicate_key)}
    protocol["overlay"] = pin("protocol-rust.json", target)
    build(root / "duplicate-check-key", "duplicate")
    protocol["graph"] = pin("protocol.json", graph)
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
