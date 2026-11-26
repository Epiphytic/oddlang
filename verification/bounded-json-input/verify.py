"""Exercise a renamed local metadata parser with a small declared budget."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
TEST = r'''
#[test]
fn bounded_cases() {
    use std::io::BufReader;
    use crate::Fault;
    assert_eq!(item(br#"{"x":1,"x":2}"#).unwrap()["x"], 2);
    assert_eq!(item(&[b' '; LIMIT + 1]), Err(Fault::Capacity));
    assert_eq!(rows(&b"{}"[..]), Err(Fault::Input));
    assert_eq!(rows(&b"{}\n{}\n"[..]).unwrap().len(), 2);
    assert_eq!(rows(&b"{}\n{}\n{\n"[..]), Err(Fault::Capacity));
    let oversized = [b'x'; LIMIT + 1];
    assert_eq!(rows(BufReader::with_capacity(3, &oversized[..])), Err(Fault::Capacity));
}
'''


def run(*argv, cwd, **kwargs):
    return subprocess.run(argv, cwd=cwd, check=True, capture_output=True, **kwargs).stdout


with tempfile.TemporaryDirectory() as path:
    root = Path(path)
    (root / "src").mkdir()
    (root / "src/lib.rs").write_text("")
    (root / "Cargo.toml").write_text(
        '[package]\nname="small-observer"\nversion="0.1.0"\nedition="2021"\n'
        '[dependencies]\nserde_json="=1.0.151"\n')
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)

    def pin(name, value=None):
        file = root / name
        if value is not None:
            file.write_text(json.dumps(value))
        return {"path": name, "sha256": hashlib.sha256(file.read_bytes()).hexdigest()}

    graph = {
        "format": "oddlang-bounded-json-input-v1", "id": "urn:test:metadata",
        "maximumValueBytes": 24, "maximumRecords": 2, "framing": "lf-terminated",
        "sizeBoundary": "include-delimiter", "duplicateMembers": "last-value",
        "guardOrder": ["byte-limit", "record-limit", "parse"],
        "capacityError": "urn:test:capacity", "invalidError": "urn:test:bad-input",
    }
    binding = {
        "format": "oddlang-bounded-json-rust-v1", "single": "item", "sequence": "rows",
        "limit": "LIMIT", "errorType": "Fault",
        "variants": {"urn:test:capacity": "Capacity", "urn:test:bad-input": "Input"},
    }
    error = {
        "format": "oddlang-finite-error-domain-v1", "id": "urn:test:errors",
        "error": "urn:test:fault", "categories": {}, "equality": True,
        "variants": {
            id: {"payload": {"kind": "unit"}, "message": {"kind": "literal", "text": text},
                 "source": False, "from": False}
            for id, text in [("urn:test:capacity", "capacity"), ("urn:test:bad-input", "bad input")]
        },
    }
    error_binding = {
        "format": "oddlang-finite-error-rust-v1",
        "variantOrder": ["urn:test:capacity", "urn:test:bad-input"],
        "categoryTypes": {}, "names": {"urn:test:fault": "Fault", **binding["variants"]},
    }
    manifest = {
        "format": "oddlang-native-package-v1", "name": "small-observer", "version": "0.1.0",
        "edition": "2021", "dependencies": {"serde_json": {"version": "1.0.151"}},
        "cargoLock": pin("Cargo.lock"), "components": [{
            "kind": "finite-error-domain", "module": "faults", "reexports": ["Fault"],
            "graph": pin("error.json", error), "overlay": pin("error-rust.json", error_binding),
        }, {
            "kind": "bounded-json-input", "module": "metadata", "reexports": [],
            "graph": pin("graph.json", graph), "overlay": pin("binding.json", binding),
        }],
    }
    pin("package.json", manifest)
    run(CLI, "build", "native-package", "rust", "package.json", "product", cwd=root)
    with (root / "product/src/group_1/__projection_1.rs").open("a") as out:
        out.write(TEST)
    result = run("cargo", "+1.97.1", "test", "--locked", "--offline", cwd=root / "product")
    assert b"1 passed" in result

    def reject(name, reason):
        pin("package.json", manifest)
        call = subprocess.run([CLI, "build", "native-package", "rust", "package.json", name],
                              cwd=root, capture_output=True, text=True)
        assert call.returncode and reason in call.stderr and not (root / name).exists(), call

    provider = manifest.pop("dependencies")
    manifest["devDependencies"] = provider
    pin("package.json", manifest)
    run(CLI, "build", "native-package", "rust", "package.json", "test-product", cwd=root)
    dev_root = root / "test-product"
    normal = run("cargo", "+1.97.1", "tree", "--locked", "--offline", "-e", "normal", cwd=dev_root)
    assert b"serde_json" not in normal
    run("cargo", "+1.97.1", "build", "--release", "--locked", "--offline", cwd=dev_root)
    with (dev_root / "src/group_1/__projection_1.rs").open("a") as out:
        out.write(TEST)
    assert b"1 passed" in run("cargo", "+1.97.1", "test", "--locked", "--offline", cwd=dev_root)
    manifest["dependencies"] = provider
    reject("duplicate-dependency", "normal and dev dependencies must have distinct names")
    del manifest["dependencies"]
    provider["serde_json"]["defaultFeatures"] = False
    reject("no-defaults", "default locked serde_json provider")
    del provider["serde_json"]["defaultFeatures"]
    binding["variants"]["urn:test:capacity"] = "Unknown"
    manifest["components"][-1]["overlay"] = pin("binding.json", binding)
    reject("unbound", "unresolved bounded JSON unit error")
    print("Renamed bounded JSON profile, record checks and linkage controls passed.")
