"""Check native links with a renamed typed parser, file policy and error domain."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def verify(cli, catalog, binding):
    package_name = "catalog-links"
    error, io = catalog["error"]["id"], "urn:test:catalog:io"
    fault, alias = "urn:test:catalog:Fault", "urn:test:catalog:Result"
    modes = {"name": {"kind": "static-text"}, "length": {"kind": "unsigned", "bits": 64},
             "mode": {"kind": "unsigned", "bits": 32}, "digest": {"kind": "bytes", "length": 32}}
    rows = [[{"kind": "literal", "text": "header", "capture": "family"}]]
    items = []
    for number, asset in enumerate(catalog["assetNames"]):
        ids = {label: f"{label}{number}" for label in modes}
        rows.append([
            {"kind": "literal", "text": asset, "capture": ids["name"]},
            {"kind": "decimal", "capture": ids["length"], "bits": 64, "minimum": 1},
            {"kind": "decimal", "capture": ids["mode"], "bits": 32, "minimum": 1},
            {"kind": "radix16", "capture": ids["digest"], "digits": 64, "decode": True},
        ])
        items.append({"record": "asset", "fields": {
            label: {"kind": "capture", "id": identifier} for label, identifier in ids.items()
        }})
    grammar = {
        "format": "oddlang-line-grammar-v1", "id": catalog["manifestGrammar"],
        "maxBytes": catalog["maximumCatalogBytes"], "forbiddenBytes": [], "rows": rows,
        "recordResult": {
            "failure": {"id": error, "code": "bad_catalog"},
            "records": {
                "book": [{"id": "family", "type": {"kind": "static-text"}},
                         {"id": "items", "type": {"kind": "record-vector", "record": "asset"}}],
                "asset": [{"id": label, "type": spec} for label, spec in modes.items()],
            },
            "entry": {"record": "book", "fields": {"family": {"kind": "capture", "id": "family"},
                                                  "items": {"kind": "vector", "items": items}}},
            "formatters": [{"id": "file", "argument": True,
                            "parts": [{"kind": "argument"}, {"kind": "literal", "text": "."},
                                      {"kind": "field", "field": "family"}]}],
        },
    }
    grammar_overlay = {
        "format": "oddlang-line-record-rust-v1", "parser": "parse", "visibility": "crate",
        "error": {"id": error, "type": ["crate", "Fault"], "variant": "Invalid"},
        "records": {key: {"name": name, "fields": {field["id"]: field["id"] for field in
                                                 grammar["recordResult"]["records"][key]}, "derives": []}
                    for key, name in (("book", "Catalog"), ("asset", "Asset"))},
        "formatters": {"file": "label"},
    }
    policy = {
        "format": "oddlang-read-only-file-v1", "maximumLinks": 1,
        "profile": {
            "id": catalog["filePolicy"], "modeMask": 511,
            "digest": {"algorithm": "sha256", "chunkBytes": 17},
            "open": ["read-only", "no-follow-final-link", "close-on-exec"],
            "guards": ["regular", "links", "maximum-size", "exact-size", "effective-owner", "masked-mode", "digest"],
            "readCeiling": "requested-size-plus-one",
            "safety": {"preflightFileType": "regular-or-final-symlink", "nonblockingOpen": True,
                       "hashCeiling": "exact-size-plus-one"},
            "errors": {**{key: {"id": error, "code": "invalid_" + code} for key, code in (
                ("descriptor", "descriptor"), ("boundedRead", "size"), ("metadata", "metadata"), ("digest", "hash"),
            )}, "io": {"id": io}},
        },
    }
    policy_overlay = {"format": "oddlang-read-only-file-rust-v1", "open": "open",
                      "read": "bounded", "verify": "verify", "error": ["crate", "Fault"],
                      "invalidVariant": "Invalid", "ioVariant": "Io"}
    errors = {
        "format": "oddlang-finite-error-domain-v1", "id": "urn:test:catalog:errors",
        "error": fault, "result": alias, "categories": {},
        "variants": {error: {"payload": {"kind": "static-text"},
                            "message": {"kind": "insertion", "prefix": "invalid: ", "suffix": ""},
                            "source": False, "from": False},
                     io: {"payload": {"kind": "io-error"}, "message": {"kind": "literal", "text": "I/O"},
                          "source": True, "from": True}},
    }
    error_overlay = {"format": "oddlang-finite-error-rust-v1", "categoryTypes": {},
                     "variantOrder": [error, io], "names": {error: "Invalid", io: "Io",
                                                          fault: "Fault", alias: "Result"}}
    with tempfile.TemporaryDirectory() as folder:
        root = Path(folder)
        (root / "src").mkdir()
        (root / "src/lib.rs").write_text("")
        (root / "Cargo.toml").write_text(
            f'[package]\nname="{package_name}"\nversion="0.1.0"\nedition="2021"\n'
            '[dependencies]\nlibc="=0.2.189"\nsha2="=0.10.9"\n')
        subprocess.run(["cargo", "+1.97.1", "generate-lockfile", "--offline"], cwd=root, check=True)

        def pin(name, value=None):
            path = root / name
            if value is not None:
                path.write_text(json.dumps(value))
            return {"path": name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

        def part(kind, module, graph, overlay, public=(), inner=()):
            return {"kind": kind, "module": module, "graph": pin(f"{module}.json", graph),
                    "overlay": pin(f"{module}-rust.json", overlay), "reexports": list(public),
                    "crateReexports": list(inner)}

        manifest = {
            "format": "oddlang-native-package-v1", "name": package_name, "version": "0.1.0",
            "edition": "2021", "cargoLock": pin("Cargo.lock"),
            "dependencies": {"libc": {"version": "0.2.189"}, "sha2": {"version": "0.10.9"}},
            "components": [
                part("finite-error-domain", "fault", errors, error_overlay, public=["Fault", "Result"]),
                part("line-grammar", "formats", grammar, grammar_overlay, inner=["Catalog"]),
                part("read-only-file", "reads", policy, policy_overlay, inner=["bounded", "verify"]),
                part("local-file-catalog", "inert", catalog, binding, inner=["Naming", "inspect_catalog"]),
            ],
        }
        path = root / "package.json"
        path.write_text(json.dumps(manifest))
        subprocess.run([cli, "build", "native-package", "rust", path, root / "product"], cwd=root, check=True)
        subprocess.run(["cargo", "+1.97.1", "check", "--offline", "--locked"], cwd=root / "product", check=True)

        with (root / "product/src/lib.rs").open("a") as source:
            source.write(r'''
#[test]
fn actual_linked_catalog() {
    use crate::inert::{Naming, inspect_catalog};
    use sha2::{Digest, Sha256};
    use std::{fs, os::unix::fs::PermissionsExt};

    let root = std::env::temp_dir().join(format!("actual-linked-catalog-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let digest = <[u8; 32]>::from(Sha256::digest(b"x"));
    let digest: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let catalog = format!("header\nrecord 1 384 {digest}\nextra 1 384 {digest}\n");
    fs::write(root.join("catalog.txt"), &catalog).unwrap();
    for name in ["record", "extra"] {
        let path = root.join(name);
        fs::write(&path, b"x").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let result = inspect_catalog(&root, Naming::Fixed, Sha256::digest(catalog.as_bytes()).into()).unwrap();
    assert_eq!(result.family, "header");
    assert_eq!(result.items.len(), 2);
    fs::remove_dir_all(root).unwrap();
}
''')
        subprocess.run(["cargo", "+1.97.1", "test", "--offline", "--locked", "--lib", "--",
                        "--exact", "actual_linked_catalog"], cwd=root / "product", check=True)

        def reject(mutate, expected):
            candidate = deepcopy(manifest)
            mutate(candidate["components"])
            path.write_text(json.dumps(candidate))
            call = subprocess.run([cli, "build", "native-package", "rust", path, root / "denied"],
                                  cwd=root, capture_output=True, text=True)
            assert call.returncode and expected in call.stderr and not (root / "denied").exists(), call

        reject(lambda parts: parts[3].update(graph=pin("bad.json", dict(catalog, manifestGrammar="urn:test:wrong"))),
               "grammar or error role differs")
        reject(lambda parts: parts[3].update(overlay=pin("bad.json", dict(binding, invalidVariant="Wrong"))),
               "grammar or error role differs")
        wrong_width = deepcopy(grammar)
        for row in wrong_width["rows"][1:]:
            row[1]["bits"] = 16
        wrong_width["recordResult"]["records"]["asset"][1]["type"]["bits"] = 16
        reject(lambda parts: parts[1].update(graph=pin("bad.json", wrong_width)), "asset field types differ")
        reject(lambda parts: parts[3].update(graph=pin("bad.json", dict(catalog, assetNames=catalog["assetNames"][::-1]))),
               "asset names differ from the grammar")
        reject(lambda parts: parts[3].update(visibility="public"), "needs private crate-scoped bindings")
        reject(lambda parts: parts.append(deepcopy(parts[2])), "file policy is ambiguous")
