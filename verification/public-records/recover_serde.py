"""Exercise source recovery with Rustdoc and two source-declared Serde policies."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
CALLER = r"""
use audit_plot::{Mark, Plot};

fn main() {
    let accepts_extra = std::env::args().nth(1).unwrap() == "open";
    let reordered = std::env::args().nth(2).unwrap() == "reordered";
    let object = r#"{"from":4,"mark":"Dry","fixed":true}"#;
    let extra = r#"{"from":4,"extra":0,"mark":"Dry","fixed":true}"#;
    let positional = if reordered { r#"[null,4,"Dry",true]"# } else { r#"[4,null,"Dry",true]"# };
    let expected = Plot { from: 4, until: None, mark: Some(Mark::Dry), fixed: true };
    let decoded = serde_json::from_str::<Plot>(object).unwrap();
    assert_eq!(decoded.clone(), expected);
    assert_eq!(serde_json::from_str::<Plot>(positional).unwrap(), expected);
    assert_eq!(serde_json::from_str::<Plot>(extra).is_ok(), accepts_extra);
    let canonical = if reordered {
        r#"{"until":null,"from":4,"mark":"Dry","fixed":true}"#
    } else {
        r#"{"from":4,"until":null,"mark":"Dry","fixed":true}"#
    };
    assert_eq!(serde_json::to_string(&decoded).unwrap(), canonical);
}
"""
SOURCE = """use serde::{Serialize, Deserialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mark { Dry, Damp }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plot {
    pub from: u32,
    pub until: Option<u32>,
    pub mark: Option<Mark>,
    pub fixed: bool,
}
"""


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(*args, failure=None, **kwargs):
    result = subprocess.run(args, check=False, capture_output=True, text=True, **kwargs)
    if failure:
        assert result.returncode and failure in result.stderr, result
    else:
        assert not result.returncode, result
    return result.stdout


with tempfile.TemporaryDirectory(prefix="oddlang-recover-plot-") as tmp:
    work = Path(tmp)
    (work / "src").mkdir()
    (work / "src/lib.rs").write_text("mod model;\npub use model::*;\n")
    (work / "Cargo.toml").write_text(
        '[package]\nname="audit_plot"\nversion="0.1.0"\nedition="2024"\n'
        '[dependencies]\nserde={version="=1.0.229",features=["derive"]}\nserde_json="=1.0.154"\n'
    )
    (work / "src/main.rs").write_text(CALLER)
    source = work / "src/model.rs"
    env = dict(os.environ, RUSTC_BOOTSTRAP="1", CARGO_TARGET_DIR=str(work / "target"))
    doc = work / "target/doc/audit_plot.json"
    selector = work / "selection.json"
    selection = {
        "format": "oddlang-serde-public-record-selection-v1",
        "sourceFilename": "src/model.rs",
        "crateRootSha256": digest(work / "src/lib.rs"),
        "crateName": "audit_plot",
        "moduleId": "urn:terrain:plot-module",
        "moduleName": "model",
        "record": {"name": "Plot", "id": "urn:terrain:plot", "fields": {
            name: f"urn:terrain:plot:{name}" for name in ("from", "until", "mark", "fixed")
        }},
        "choices": [{"name": "Mark", "id": "urn:terrain:mark"}],
    }

    def document(rust):
        source.write_text(rust)
        run("cargo", "+1.97.1", "rustdoc", "--offline", "--lib", "--",
            "-Z", "unstable-options", "--output-format", "json", "--document-private-items",
            cwd=work, env=env)

    def recover(name, expected_failure=None, **overrides):
        selector.write_text(json.dumps(dict(selection, sourceSha256=digest(source),
                                            rustdocSha256=digest(doc)) | overrides))
        output = work / name
        run(CLI, "recover", "serde-public-record", source, doc, selector, output,
            failure=expected_failure, cwd=work)
        assert output.exists() != bool(expected_failure)
        if not expected_failure:
            return tuple(json.loads((output / file).read_text()) for file in ("graph.json", "rust.json"))

    def check_native(label, policy, order):
        model = work / label
        run(CLI, "build", "public-records", "rust", model / "graph.json", model / "rust.json",
            model / "projected", cwd=work)
        original = source.read_text()
        args = ("cargo", "+1.97.1", "run", "--offline", "--", policy, order)
        run(*args, cwd=work, env=env)
        enum = SOURCE.split("#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\n"
                            "pub struct", 1)[0]
        source.write_text(enum + (model / "projected/records.rs").read_text())
        try:
            run(*args, cwd=work, env=env)
        finally:
            source.write_text(original)

    document(SOURCE)
    baseline, binding = recover("baseline")
    record = baseline["modules"][0]["records"][0]
    assert record["traits"] == ["clone", "debug", "partial-eq", "eq", "serialize", "deserialize"]
    assert [field["wire"] for field in record["fields"]] == ["from", "until", "mark", "fixed"]
    assert [field["type"] for field in record["fields"]] == [
        "u32", {"optional": "u32"}, {"optional": {"external": "urn:terrain:mark"}}, "bool"
    ]
    assert binding["externalTypes"] == {"urn:terrain:mark": ["crate", "Mark"]}
    assert record["readPolicy"] == {"shape": "object-or-ordered-sequence", "unknownFields": "ignore"}
    recover("wrong-type", "field declaration", choices=[{"id": "urn:terrain:mark", "name": "Plot"}])
    recover("stale", "digest", sourceSha256="0" * 64)
    recover("stale-root", "digest", crateRootSha256="0" * 64)
    check_native("baseline", "open", "initial")

    document(SOURCE.replace("pub struct Plot", "#[serde(deny_unknown_fields)]\npub struct Plot"))
    mutated, mutated_binding = recover("mutated")
    record["readPolicy"]["unknownFields"] = "reject"
    assert mutated == baseline and mutated_binding == binding
    check_native("mutated", "closed", "initial")

    document(SOURCE.replace("    pub from: u32,\n    pub until: Option<u32>,",
                            "    pub until: Option<u32>,\n    pub from: u32,"))
    reordered, reordered_binding = recover("reordered")
    record["readPolicy"]["unknownFields"] = "ignore"
    record["fields"][0], record["fields"][1] = record["fields"][1], record["fields"][0]
    assert reordered == baseline and reordered_binding == binding
    check_native("reordered", "open", "reordered")

    document(SOURCE.replace("pub struct Plot", '#[serde(rename_all = "camelCase")]\npub struct Plot'))
    recover("unsupported-policy", "unsupported Serde record attribute")

    document(SOURCE.replace("pub struct Plot", "pub type Option<T> = Vec<T>;\n\n"
                            "#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub struct Plot")
             .replace("#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\npub type",
                      "pub type"))
    recover("shadowed-option", "shadowed type")

    (work / "src/lib.rs").write_text("mod model;\npub use model::Plot;\n")
    document(SOURCE)
    recover("missing-root-export", "expected one crate-root export of Mark",
            crateRootSha256=digest(work / "src/lib.rs"))

    for label, shadow in (
        ("private-root-record", "struct Plot;"),
        ("private-root-choice", "mod other { pub struct Mark; }\nuse other::Mark;"),
    ):
        (work / "src/lib.rs").write_text(f"mod model;\npub use model::*;\n{shadow}\n")
        document(SOURCE)
        recover(label, "crate-root export", crateRootSha256=digest(work / "src/lib.rs"))

print("Recovered default and strict Serde policy, source field order, and compiler-checked choice links")
