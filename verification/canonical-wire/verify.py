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
protocol["errors"] = {}
rust_errors = {}
for identity, binding_info in target["errors"].items():
    variant = f"{identity}:variant"
    protocol["errors"][identity] = {"variant": variant, "argument": binding_info.get("argument")}
    rust_errors[variant] = binding_info["variant"]
target["errors"] = rust_errors
protocol["digests"] = [
    {"id": "urn:radio:raw-digest", "kind": "bytes"},
    {"id": "urn:radio:tx-digest", "kind": "case", "union": message, "variant": send,
     "parameters": [{"id": "urn:radio:level-arg", "field": "urn:radio:send:gain", "passing": "owned"}],
     "steps": []},
]
target["auxiliary"] = {"urn:radio:raw-digest": "raw_digest", "urn:radio:tx-digest": "transmit_digest",
                       "urn:radio:level-arg": "gain"}

def step(kind, name, **members):
    return {"kind": kind, "id": f"urn:radio:{name}", **members}


def reference(id):
    return {"kind": "ref", "id": id}


def require(name, predicate, error="range"):
    return step("require", name, predicate=predicate, error=f"urn:radio:{error}")


def bound(left, right):
    return {"kind": "compare", "left": left, "operator": "at-most", "right": right}


def integer(value):
    return {"kind": "constant", "value": value}


sweep = "urn:radio:sweep"
burst = "urn:radio:burst"
calibration = "urn:radio:cal"
probe = "urn:radio:probe"
scan = "urn:radio:scan"
check = "urn:radio:probe-check"
estimate = "urn:radio:probe-estimate"
samples, sensor, token = "urn:radio:burst:samples", "urn:radio:burst:sensor", "urn:radio:burst:token"
policy = {"maxItems": 4, "maxItemBytes": 3, "maxTotalBytes": 8, "perItemOverhead": 1,
          "listLimitMessage": "too many samples", "encodedLimitMessage": "too many encoded bytes",
          "decodedLimitMessage": "too many bytes", "syntaxMessage": "bad samples"}
hex16 = {"kind": "lower-hex", "bytes": 16, "diagnostic": "radio hex"}
shape["types"][1]["variants"].append(
    {"id": scan, "name": "scan", "payload": {"kind": "newtype", "type": reference(sweep)}})
shape["types"].extend([
    {"kind":"external", "id":probe},
    {"kind":"adjacent", "id":sweep, "tag":"kind", "content":"data", "variants":[
        {"id":burst, "name":"burst", "payload":{"kind":"record", "fields":[
            field("burst", "samples", {"kind": "bounded-base64-list", "policy": policy}),
            field("burst", "sensor", {"kind": "optional", "item": reference(probe)}),
            field("burst", "token", hex16),
        ]}},
        {"id":calibration, "name":"cal", "payload":{"kind":"record", "fields":[]}},
    ]},
])
binding["names"].update({sweep:"Sweep", scan:"Scan", burst:"Burst", calibration:"Calibration",
                         samples:"samples", sensor:"sensor", token:"token"})
binding["derives"][sweep] = ["Clone", "PartialEq", "Eq"]
binding["externals"][probe] = "crate::Probe"
binding["externalDerives"] = {probe: ["Clone", "PartialEq", "Eq"]}
scan_id, plan_field, seq_field = "urn:radio:scan-operation", "urn:radio:scan-plan", "urn:radio:scan-seq"
protocol["maximumWireBytes"] = 240
protocol["endpoints"].append({
    "id":scan_id, "variant":scan, "guards":[],
    "fields": [
        {"id":seq_field, "from":{"kind":"envelope-field", "field":"urn:radio:envelope:sequence"}},
        {"id":plan_field, "from":{"kind":"case-value"}},
    ],
    "steps":[step("cases", "sweep-guards", value=operand("value", plan_field), arms=[{
        "variant":burst, "bind":"case", "steps":[
            require("unique-samples", {"kind":"unique", "value":operand("case", samples)}),
            require("sample-budget", {"kind":"total-bytes", "value":operand("case", samples),
                                        "overhead":1, "maximum":4}, "size"),
            require("sample-token", bound(
                {"kind":"big-endian-prefix", "value":operand("case", token), "bytes":1}, integer(8))),
            step("some", "probe-present", value=operand("case", sensor), bind="present", steps=[
                step("call", "probe-call", value=operand("present"), method=check),
                step("on-success", "probe-estimate-call", value=operand("present"), method=estimate,
                     bind="sample", steps=[require("probe-bound", bound(
                         {"kind":"field", "value":operand("sample")}, integer(3)))]),
            ]),
        ],
    }])],
})
target["fields"].update({plan_field:"plan", seq_field:"sequence"})
target["endpoints"][scan_id] = {"type":"Scan", "encode":"encode_scan","decode":"decode_scan",
                                 "derives":["Clone","PartialEq","Eq"]}
rx["timed"] = [require("reply-clock", bound({"kind":"field","value":operand("clock")}, integer(10)), "early")]
target["endpoints"][rx["id"]]["timed"] = "validate_at"
protocol["foreign"] = [{"id":probe,"fields":[],"variants":[],"methods":[
    {"id":check,"returns":"unit"},{"id":estimate,"returns":"unsigned"}]}]
target["foreignMembers"] = {check:"permit", estimate:"estimate"}
protocol["aliases"] = [{"id":"urn:radio:token-alias","bytes":16}]
protocol["hexExports"] = [{"id":"urn:radio:token-encoding","bytes":16,"diagnostic":"radio hex"}]
cal_hash, burst_hash = "urn:radio:cal-hash", "urn:radio:burst-hash"
protocol["digests"].extend([
    {"id":cal_hash,"kind":"case","union":sweep,"variant":calibration,"parameters":[],"steps":[]},
    {"id":burst_hash,"kind":"case","union":sweep,"variant":burst,"parameters":[
        {"id":"urn:radio:samples-arg","field":samples,"passing":"borrowed-list"},
        {"id":"urn:radio:sensor-arg","field":sensor,"passing":"owned"},
        {"id":"urn:radio:token-arg","field":token,"passing":"owned"},
    ],"steps":[require("digest-sample-budget", {
        "kind":"total-bytes", "value":operand("value", "urn:radio:samples-arg"),
        "overhead":1, "maximum":4,
    }, "size")]},
])
target["auxiliary"].update({"urn:radio:token-alias":"Token","urn:radio:token-encoding":"token_hex",
    cal_hash:"cal_digest", burst_hash:"burst_digest", "urn:radio:samples-arg":"budget",
    "urn:radio:sensor-arg":"sensor", "urn:radio:token-arg":"token"})
target["internalEnvelope"] = True

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
    altered_target = json.loads(overlay.read_bytes())
    del altered_target["endpoints"]["urn:radio:tx"]["timed"]
    run(*common, write(temp / "clock.json", bad), write(temp / "clock-overlay.json", altered_target),
        temp / "clock", failure="unknown guard scope clock")
    bad = json.loads(graph.read_bytes())
    bad["endpoints"][0]["timed"][0]["id"] = "urn:radio:gain-check"
    run(*common, write(temp / "duplicate.json", bad), overlay, temp / "duplicate",
        failure="duplicate wire declaration")
    bad = json.loads(graph.read_bytes())
    bad["digests"][0]["id"] = "urn:radio:tx"
    run(*common, write(temp / "duplicate-digest.json", bad), overlay, temp / "digest",
        failure="duplicate wire declaration")
    public_shape, native_shape = json.loads(shape_path.read_bytes()), json.loads(shape_binding.read_bytes())
    public_shape["types"].append({"kind":"record", "id":"urn:radio:outer",
                                  "fields":[field("outer", "ref", reference(message))]})
    native_shape["names"].update({"urn:radio:outer":"Outer", "urn:radio:outer:ref":"frame"})
    run("build", "wire-protocol", "rust", write(temp / "public.json", public_shape),
        write(temp / "public-overlay.json", native_shape), graph, overlay, temp / "leak",
        failure="public structural type refers to the internal frame")
    bad = json.loads(shape_path.read_bytes())
    bad["guards"].append({"id":"urn:radio:plain-envelope-check","owner":env,
        "require":{"kind":"unsigned-range","field":"urn:radio:envelope:sequence",
                   "minimum":1,"maximum":20},"error":"urn:radio:range"})
    run("build", "wire-protocol", "rust", write(temp / "private-guard.json", bad), shape_binding,
        graph, overlay, temp / "invisible-guard", failure="internal frame cannot own")
    single_shape, single_binding = json.loads(shape_path.read_bytes()), json.loads(shape_binding.read_bytes())
    single_shape["types"][1]["variants"] = [single_shape["types"][1]["variants"][0]]
    for removed in (scan, read):
        del single_binding["names"][removed]
    single_protocol, single_target = json.loads(graph.read_bytes()), json.loads(overlay.read_bytes())
    single_protocol["endpoints"] = single_protocol["endpoints"][:1]
    only_endpoint = single_protocol["endpoints"][0]
    single_target["endpoints"] = {only_endpoint["id"]: single_target["endpoints"][only_endpoint["id"]]}
    single_target["fields"] = {field["id"]: single_target["fields"][field["id"]]
                               for field in only_endpoint["fields"]}
    singleton = temp / "singleton"
    run("build", "wire-protocol", "rust", write(temp / "single-shape.json", single_shape),
        write(temp / "single-binding.json", single_binding),
        write(temp / "single-protocol.json", single_protocol),
        write(temp / "single-target.json", single_target), singleton)
    shape_only_graph = json.loads(graph.read_bytes())
    shape_only_graph["endpoints"][2]["steps"] = []
    shape_only_graph["digests"][-1]["steps"] = []
    shape_only = temp / "shape-only"
    run(*common, write(temp / "shape-only-protocol.json", shape_only_graph), overlay, shape_only)
    consumer = temp / "consumer"
    shutil.copytree(ROOT / "fixture", consumer)
    shutil.copyfile(output / "wire.rs", consumer / "src/generated.rs")
    shutil.copyfile(singleton / "wire.rs", consumer / "src/singleton.rs")
    shutil.copyfile(shape_only / "wire.rs", consumer / "src/shape_only.rs")
    subprocess.run(("cargo", "+1.97.1", "run", "--locked", "--offline", "--quiet"), cwd=consumer,
                   env={**os.environ, "CARGO_ENCODED_RUSTFLAGS": "-Dwarnings"}, check=True)
print("canonical source-free native wire operations passed")
