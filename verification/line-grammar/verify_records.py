"""Compile unrelated records with typed projections and independent rejection cases."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent


def run(*args, env=None, success=True):
    p = subprocess.run(args, env=env, capture_output=True, text=True, timeout=120)
    if (p.returncode == 0) != success:
        raise ValueError(f"{args}\n{p.stdout}{p.stderr}")
    return p.stderr


def literal(text, capture=None):
    return dict(kind="literal", text=text, capture=capture)


def decimal(name, bits=64):
    return dict(kind="decimal", capture=name, bits=bits, minimum=1)


def captures(**fields):
    return {field: {"kind": "capture", "id": capture} for field, capture in fields.items()}


def types(**fields):
    return [{"id": field, "type": ty} for field, ty in fields.items()]


scope = "urn:inventory:bad-log"
graph = {
    "format": "oddlang-line-grammar-v1",
    "id": "urn:inventory:structured-log",
    "maxBytes": 128,
    "forbiddenBytes": [9, 13],
    "rows": [
        [literal("log"), literal("2")],
        [literal("mode"), {"kind": "text", "capture": "mode", "choices": ["indoor", "outdoor"]},
         {"kind": "boolean", "capture": "active", "trueToken": "active", "falseToken": "inactive"}],
        [literal("rows"), decimal("countA", 16), decimal("countB", 16)],
        [literal("item"), literal("top", "nameA"), decimal("amountA")],
        [literal("item"), literal("lower", "nameB"), decimal("amountB")],
    ],
    "constraints": [
        {"kind": "allowed-tuples", "captures": ["mode", "active"],
         "allowed": [["indoor", True], ["outdoor", False]], "afterRow": 2},
        {"kind": "checked-sum", "captures": ["amountA", "amountB"], "maximum": 16, "afterRow": 5},
    ],
    "recordResult": {
        "failure": {"id": scope, "code": "bad_log"},
        "records": {
            "book": types(mode={"kind": "owned-text"},
                          counts={"kind": "unsigned-array", "bits": 16, "length": 2},
                          samples={"kind": "record-vector", "record": "sample"}),
            "sample": types(name={"kind": "static-text"},
                            amount={"kind": "unsigned", "bits": 64}),
        },
        "entry": {"record": "book", "fields": captures(mode="mode") | {
            "counts": {"kind": "array", "captures": ["countA", "countB"]},
            "samples": {"kind": "vector", "items": [
                {"record": "sample", "fields": captures(name="nameA", amount="amountA")},
                {"record": "sample", "fields": captures(name="nameB", amount="amountB")},
            ]},
        }},
        "formatters": [
            {"id": "key", "argument": False, "parts": [{"kind": "field", "field": "mode"}]},
            {"id": "resource", "argument": True, "parts": [
                {"kind": "argument"}, {"kind": "literal", "text": ":"},
                {"kind": "field", "field": "mode"},
            ]},
        ],
    },
}
overlay = {
    "format": "oddlang-line-record-rust-v1",
    "parser": "parse",
    "visibility": "crate",
    "error": {"id": scope, "type": ["crate", "Fault"], "variant": "Invalid"},
    "records": {
        id: {"name": name, "fields": {f["id"]: f["id"] for f in graph["recordResult"]["records"][id]},
             "derives": ["clone", "debug", "partial-eq", "eq"]}
        for id, name in (("book", "LogBook"), ("sample", "Sample"))
    },
    "formatters": {"key": "key", "resource": "resource"},
}
with tempfile.TemporaryDirectory(prefix="odd-line-records-") as tmp:
    stage = Path(tmp)
    g, r, out = [stage / p for p in ("graph.json", "rust.json", "generated")]
    g.write_text(json.dumps(graph))
    r.write_text(json.dumps(overlay))
    run(CLI, "build", "line-grammar", "rust", g, r, out)
    exe = stage / "caller"
    run("rustc", "+1.97.1", "--edition=2021", HERE / "records.rs", "-o", exe,
        env=dict(os.environ, GENERATED=str(out / "lines.rs")))
    run(exe)
    def pin(path):
        file = stage / path
        return {"path": path, "sha256": hashlib.sha256(file.read_bytes()).hexdigest()}

    error_graph = stage / "errors.json"
    error_overlay = stage / "errors-rust.json"
    error_id, alias_id = "urn:inventory:Fault", "urn:inventory:Result"
    error_graph.write_text(json.dumps({
        "format": "oddlang-finite-error-domain-v1",
        "id": "urn:inventory:error-domain",
        "error": error_id, "result": alias_id, "categories": {},
        "variants": {scope: {"payload": {"kind": "static-text"},
                             "message": {"kind": "insertion", "prefix": "invalid: ", "suffix": ""},
                             "source": False, "from": False}},
    }))
    error_overlay.write_text(json.dumps({
        "format": "oddlang-finite-error-rust-v1", "categoryTypes": {},
        "variantOrder": [scope], "names": {error_id: "Fault", alias_id: "Result", scope: "Invalid"},
    }))
    (stage / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "inventory-log"\nversion = "0.0.0"\n')
    package = {
        "format": "oddlang-native-package-v1", "name": "inventory-log",
        "version": "0.0.0", "edition": "2021", "dependencies": {}, "cargoLock": pin("Cargo.lock"),
        "components": [
            {"kind": "finite-error-domain", "module": "faults", "graph": pin(error_graph.name),
             "overlay": pin(error_overlay.name), "reexports": ["Fault", "Result"]},
            {"kind": "line-grammar", "module": "logbook", "graph": pin(g.name), "overlay": pin(r.name),
             "reexports": [], "crateReexports": ["LogBook", "Sample"]},
        ],
    }
    manifest = stage / "package.json"
    manifest.write_text(json.dumps(package))
    output = stage / "inventory-product"
    run(CLI, "build", "native-package", "rust", manifest, output)
    run("cargo", "+1.97.1", "build", "--offline", "--locked", "--manifest-path", output / "Cargo.toml")
    overlay["error"]["variant"] = "Incorrect"
    r.write_text(json.dumps(overlay))
    package["components"][1]["overlay"] = pin(r.name)
    manifest.write_text(json.dumps(package))
    rejected_package = stage / "rejected-package"
    denial = run(CLI, "build", "native-package", "rust", manifest, rejected_package, success=False)
    assert "error binding does not match a native export" in denial and not rejected_package.exists()
    overlay["error"]["variant"] = "Invalid"
    r.write_text(json.dumps(overlay))

    graph["constraints"][1]["afterRow"] = 4
    g.write_text(json.dumps(graph))
    rejected = stage / "rejected"
    stderr = run(CLI, "build", "line-grammar", "rust", g, r, rejected, success=False)
    assert "unavailable capture" in stderr and not rejected.exists()
print("Typed records, finite relation, checked aggregate, and formatters passed")
