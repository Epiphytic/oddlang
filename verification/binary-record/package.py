#!/usr/bin/env python3
"""Exercise typed nested linkage in a generated geological file format."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


CLI = Path(sys.argv[1]).resolve()
CARGO = ("cargo", "+1.97.1")
RECORD = "urn:geology:sample"
LAYER = "urn:geology:layer"
DEPTH = "urn:geology:depth"
MARK = "urn:geology:mark"
NESTED = "urn:geology:sample:layer"
FAILURE = "urn:geology:error:failure"
FAULT = "urn:geology:error:invalid"
OVERFLOW = "urn:geology:error:overflow"


def encoding(kind, **values):
    return {"kind": kind, **values}


def slot(identity, shape):
    return {"id": identity, "encoding": shape}


def graph(identity, maximum, layout):
    return {
        "format": "oddlang-binary-record-v1", "id": identity,
        "maximum": maximum, "layout": layout,
        "errors": dict.fromkeys(("value", "truncated", "trailing", "decodeSize", "encodeSize"), FAULT),
    }


inner = graph(LAYER, 6, [
    slot(DEPTH, encoding("signed-be", bits=16)),
    slot(MARK, encoding("fixed-bytes", size=4)),
])
outer = graph(RECORD, 12, [
    slot("urn:geology:sample:magic", encoding("literal", bytes=[71, 1], error=FAULT)),
    slot(NESTED, encoding("nested", record=LAYER, maximum=6, overflow=OVERFLOW)),
])
outer["externalRecords"] = {
    LAYER: {"fields": {DEPTH: encoding("signed", bits=16), MARK: encoding("fixed-bytes", size=4)}}
}


def binding(name, names, external_records=None):
    result = {
        "format": "oddlang-binary-record-rust-v1", "name": name, "names": names,
        "externals": {}, "errorType": "crate::Failure",
        "errors": {FAULT: {"variant": "Invalid"}},
    }
    if external_records:
        result["externalRecords"] = external_records
    if name == "Sample":
        result["errors"][OVERFLOW] = {"variant": "Overflow"}
    return result


inner_binding = binding("Layer", {DEPTH: "depth", MARK: "mark"})
outer_binding = binding("Sample", {NESTED: "layer"}, {
    LAYER: {"path": "crate::layers::Layer", "fields": {DEPTH: "depth", MARK: "mark"}}
})
fault_graph = {
    "format": "oddlang-shared-declarations-v1", "id": "urn:geology:errors",
    "errors": [{"id": FAILURE, "variants": [
        {"id": FAULT, "payload": "none", "message": [{"text": "invalid layer"}]},
        {"id": OVERFLOW, "payload": "none", "message": [{"text": "layer too long"}]},
    ]}],
}
fault_binding = {
    "format": "oddlang-shared-rust-v1", "names": {FAILURE: "Failure", FAULT: "Invalid", OVERFLOW: "Overflow"},
    "variantOrder": {FAILURE: [FAULT, OVERFLOW]},
}


def run(*args, cwd=None, failure=None, env=None):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True)
    if failure:
        assert result.returncode and failure in result.stderr, result
    else:
        assert not result.returncode, result


def pin_file(path):
    return {"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


with tempfile.TemporaryDirectory(prefix="oddlang-composite-package-") as directory:
    root = Path(directory)
    env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    (root / "Cargo.toml").write_text(
        '[package]\nname="geology"\nversion="0.0.0"\nedition="2024"\n'
        '[lib]\npath="placeholder.rs"\n[workspace]\n'
    )
    run(*CARGO, "generate-lockfile", "--offline", cwd=root, env=env)

    def pin(name, value):
        path = root / f"{name}.json"
        path.write_text(json.dumps(value))
        return pin_file(path)

    def component(kind, module, exports, shape, overlay, roots=None):
        item = {
            "kind": kind, "module": module, "reexports": exports,
            "graph": pin(module + "-graph", shape), "overlay": pin(module + "-rust", overlay),
        }
        if roots is not None:
            item["rootReexports"] = roots
        return item

    faults = component("shared-declarations", "faults", ["Failure"], fault_graph, fault_binding)

    def build(label, *, provide=True, inside=inner, inside_binding=inner_binding,
              outside=outer, outside_binding=outer_binding, roots=(), extra=False,
              stale_pin=False, failure=None):
        parts = [faults, component("binary-record", "samples", ["Sample"], outside, outside_binding)]
        if provide:
            codec = component("binary-record", "layers", ["Layer"], inside, inside_binding, list(roots))
            parts.append(codec)
            if stale_pin:
                path = root / codec["graph"]["path"]
                path.write_bytes(path.read_bytes() + b"\n")
        if extra:
            parts.append(component("binary-record", "other", ["Layer"], inside, inside_binding, []))
        manifest = root / "package.json"
        manifest.write_text(json.dumps({
            "format": "oddlang-native-package-v1", "name": "geology", "version": "0.0.0",
            "edition": "2024", "cargoLock": pin_file(root / "Cargo.lock"), "components": parts,
        }))
        output = root / label
        run(CLI, "build", "native-package", "rust", manifest, output, failure=failure, env=env)
        assert output.exists() != bool(failure), output
        return output

    output = build("accepted")
    repeat = build("repeat")
    assert (output / "package.lock").read_bytes() == (repeat / "package.lock").read_bytes()
    assert {p.relative_to(output): p.read_bytes() for p in output.rglob("*.rs")} == {
        p.relative_to(repeat): p.read_bytes() for p in repeat.rglob("*.rs")
    }
    (output / "tests").mkdir()
    (output / "tests" / "native.rs").write_text("""
use geology::{Failure, Sample};

#[test]
fn observes_file_bytes_and_bound() {
    let wire = [71, 1, 0, 0, 0, 6, 255, 254, 40, 41, 42, 43];
    let sample = Sample::decode(&wire).unwrap();
    assert_eq!(sample.layer.depth, -2i16);
    assert_eq!(sample.layer.mark, [40, 41, 42, 43]);
    assert_eq!(sample.encode().unwrap(), wire);
    // The slot rejects the length before attempting a payload read.
    assert!(matches!(Sample::decode(&[71, 1, 0, 0, 0, 7]), Err(Failure::Overflow)));
    assert!(matches!(Sample::decode(&[71, 1, 0, 0, 0, 6]), Err(Failure::Invalid)));
}
""")
    run(*CARGO, "test", "--locked", "--offline", cwd=output, env=env)

    narrowed = copy.deepcopy(outer)
    narrowed["layout"][1]["encoding"]["maximum"] = 5
    build("short-cap", outside=narrowed, failure="nested codec minimum exceeds slot")
    wider = copy.deepcopy(outer)
    wider["layout"][1]["encoding"]["maximum"] = 7
    build("wide-cap", outside=wider)
    build("missing", provide=False, failure="nested codec identity has no pinned package export")
    build("duplicate", extra=True, failure="ambiguous pinned nested codec")
    renamed = copy.deepcopy(inner_binding)
    renamed["names"][DEPTH] = "position"
    build("renamed", inside_binding=renamed, failure="nested codec member mismatch")
    wrong_width = copy.deepcopy(inner)
    wrong_width["layout"][0]["encoding"]["bits"] = 8
    build("sign-width", inside=wrong_width, failure="nested codec member mismatch")
    wrong_array = copy.deepcopy(inner)
    wrong_array["layout"][1]["encoding"]["size"] = 3
    build("array-width", inside=wrong_array, failure="nested codec member mismatch")
    wrong_sign = copy.deepcopy(inner)
    wrong_sign["layout"][0]["encoding"] = encoding("unsigned-be", bits=16, minimum=0, maximum=15)
    build("unsigned", inside=wrong_sign, failure="nested codec member mismatch")
    build("untrusted-pin", stale_pin=True, failure="input digest mismatch")
    wrong_identity = copy.deepcopy(inner)
    wrong_identity["id"] = "urn:geology:other-layer"
    build("wrong-identity", inside=wrong_identity, failure="nested codec identity mismatch")
    missing_member = copy.deepcopy(outer)
    missing_member["externalRecords"][LAYER]["fields"]["urn:geology:unknown"] = (
        missing_member["externalRecords"][LAYER]["fields"].pop(MARK)
    )
    missing_binding = copy.deepcopy(outer_binding)
    missing_binding["externalRecords"][LAYER]["fields"]["urn:geology:unknown"] = (
        missing_binding["externalRecords"][LAYER]["fields"].pop(MARK)
    )
    build("missing-member", outside=missing_member, outside_binding=missing_binding,
          failure="nested codec member mismatch")
    fragment = copy.deepcopy(inner)
    fragment["fragment"] = True
    build("standalone-fragment", inside=fragment)
    circular = copy.deepcopy(inner)
    circular["maximum"] = 12
    circular["layout"].append(slot("urn:geology:cycle", encoding(
        "nested", record=RECORD, maximum=12, overflow=FAULT,
    )))
    circular["externalRecords"] = {RECORD: {"fields": {}}}
    circular_binding = copy.deepcopy(inner_binding)
    circular_binding["names"]["urn:geology:cycle"] = "cycle"
    circular_binding["externalRecords"] = {RECORD: {"path": "crate::Sample", "fields": {}}}
    connects = copy.deepcopy(outer)
    connects["layout"][1]["encoding"]["maximum"] = 12
    build("cycle", inside=circular, inside_binding=circular_binding, outside=connects,
          failure="cyclic required nested codec slots")
    wrong_path = copy.deepcopy(outer_binding)
    wrong_path["externalRecords"][LAYER]["path"] = "crate::Layer"
    build("unexported", outside_binding=wrong_path, failure="nested codec identity has no pinned package export")
    build("root-export", outside_binding=wrong_path, roots=("Layer",))

print("Pinned nested file-format package, exact bytes, and linkage controls pass.")
