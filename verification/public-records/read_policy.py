"""Exercise open and default-closed data records in a renamed native package."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
MODULE = "urn:fixture:collection"
CHOICE = "urn:fixture:state"
record = {
    "id": MODULE + ":record", "traits": ["clone", "debug", "partial-eq", "eq", "serialize", "deserialize"],
    "readPolicy": {"shape": "object-or-ordered-sequence", "unknownFields": "ignore"},
    "fields": [{"id": MODULE + ":" + name, "wire": name, "type": ty} for name, ty in (
        ("index", "u64"), ("label", {"optional": "text"}), ("state", {"external": CHOICE}),
    )],
}
graph = {"format": "oddlang-public-records-v1",
         "modules": [{"id": MODULE, "records": [record], "aliases": []}]}
binding = {"format": "oddlang-public-records-rust-v1", "flattenModule": True,
           "identifiers": {MODULE: "catalog", record["id"]: "Sample"},
           "externalTypes": {CHOICE: ["crate", "State"]}}
choice = {"format": "oddlang-shared-declarations-v1", "id": "urn:fixture:choice-domain",
          "stringEnums": [{"id": CHOICE,
                           "variants": [{"id": CHOICE + ":collected", "wire": "collected"}]}]}
choice_binding = {"format": "oddlang-shared-rust-v1", "variantOrder": {CHOICE: [CHOICE + ":collected"]},
                  "names": {CHOICE: "State", CHOICE + ":collected": "Collected"}}

TEST = r'''
#[test]
fn wire_policy_examples() {
    use crate::{Sample, State};

    let unknown = r#"{"index":4,"state":"collected","extra":9}"#;
    let known = r#"{"state":"collected","index":4}"#;
    let array = r#"[4,null,"collected"]"#;
    let expected = Sample { index: 4, label: None, state: State::Collected };
    assert_eq!(::serde_json::from_str::<Sample>(known).unwrap(), expected);
    assert_eq!(::serde_json::from_str::<Sample>(array).unwrap(), expected);
    assert_eq!(::serde_json::to_string(&expected.clone()).unwrap(), r#"{"index":4,"label":null,"state":"collected"}"#);
    assert!(::serde_json::from_str::<Sample>(r#"{"index":4,"index":5,"state":"collected"}"#).is_err());
    if CLOSED {
        assert!(::serde_json::from_str::<Sample>(unknown).unwrap_err().to_string().starts_with("unknown field"));
    } else {
        assert_eq!(::serde_json::from_str::<Sample>(unknown).unwrap(), expected);
    }
}
'''


def run(*cmd, cwd):
    return subprocess.run(cmd, cwd=cwd, check=True)


with tempfile.TemporaryDirectory() as work:
    root = Path(work)
    (root / "src").mkdir()
    (root / "src/lib.rs").write_text("")
    (root / "Cargo.toml").write_text(
        '[package]\nname="data-record-fixture"\nversion="0.1.0"\nedition="2021"\n'
        '[dependencies]\nserde={ version="=1.0.229", features=["derive"] }\nserde_json="=1.0.151"\n')
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)

    def pin(file, value=None):
        path = root / file
        if value is not None:
            path.write_text(json.dumps(value))
        return {"path": file, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def manifest(records, overlay):
        return {
            "format": "oddlang-native-package-v1", "name": "data-record-fixture", "version": "0.1.0",
            "edition": "2021", "cargoLock": pin("Cargo.lock"),
            "dependencies": {"serde": {"version": "1.0.229", "features": ["derive"]},
                             "serde_json": {"version": "1.0.151"}},
            "components": [
                {"kind": "shared-declarations", "module": "state_defs", "reexports": ["State"],
                 "graph": pin("choice.json", choice), "overlay": pin("choice-rust.json", choice_binding)},
                {"kind": "public-records", "module": "records", "reexports": ["Sample"],
                 "graph": pin("record.json", records), "overlay": pin("record-rust.json", overlay)},
            ],
        }

    for closed in (False, True):
        current = deepcopy(graph)
        if closed:
            del current["modules"][0]["records"][0]["readPolicy"]
        pin("package.json", manifest(current, binding))
        product = root / ("closed" if closed else "open")
        run(CLI, "build", "native-package", "rust", root / "package.json", product, cwd=root)
        with (product / "src/lib.rs").open("a") as program:
            program.write(TEST.replace("CLOSED", str(closed).lower()))
        run("cargo", "+1.97.1", "test", "--offline", "--locked", "--lib", "--",
            "--exact", "wire_policy_examples", cwd=product)

    wrong = dict(binding, externalTypes={CHOICE: ["crate", "Other"]})
    pin("package.json", manifest(graph, wrong))
    denied = subprocess.run([CLI, "build", "native-package", "rust", root / "package.json", root / "denied"],
                            cwd=root, capture_output=True, text=True)
    assert denied.returncode and "external public record choice" in denied.stderr and not (root / "denied").exists()

    for altered, field, value, diagnostic in (
        ("policy-without-deserializer", "traits", ["serialize"], "read policy requires Deserialize"),
        ("unsafe-input-shape", "readPolicy", {"shape": "unordered-sequence", "unknownFields": "ignore"},
         "unknown variant"),
        ("equality-without-partial", "traits", ["eq", "deserialize"], "Eq requires PartialEq"),
    ):
        invalid = deepcopy(graph)
        invalid["modules"][0]["records"][0][field] = value
        pin("package.json", manifest(invalid, binding))
        denied = subprocess.run([CLI, "build", "native-package", "rust", root / "package.json", root / altered],
                                cwd=root, capture_output=True, text=True)
        assert denied.returncode and diagnostic in denied.stderr and not (root / altered).exists()
