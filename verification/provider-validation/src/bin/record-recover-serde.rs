//! Exercise Rustdoc recovery, native field order and source-declared Serde policies.
use provider_validation::{Result, cargo, cli, command, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{fs, process::Command};

const SOURCE: &str = include_str!("../../fixtures/public-records/recover_serde/source.rs");
const ROOT: &str = "mod model;\npub use model::*;\n";
const DERIVE: &str = "#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]";

fn main() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    fs::create_dir(root.join("src"))?;
    let lib = root.join("src/lib.rs");
    let source = root.join("src/model.rs");
    let target = root.join("doc-target");
    fs::write(&lib, ROOT)?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='audit_plot'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nserde={version='=1.0.229',features=['derive']}\nserde_json='=1.0.154'\n",
    )?;
    fs::write(
        root.join("src/main.rs"),
        include_bytes!("../../fixtures/public-records/recover_serde/caller.rs"),
    )?;
    let fields = ["from", "until", "mark", "fixed"]
        .into_iter()
        .map(|name| (name.to_string(), json!(format!("urn:terrain:plot:{name}"))))
        .collect::<serde_json::Map<_, _>>();
    let selection = json!({"format":"oddlang-serde-public-record-selection-v1","sourceFilename":"src/model.rs",
        "crateRootSha256":sha(&lib)?,"crateName":"audit_plot","moduleId":"urn:terrain:plot-module","moduleName":"model",
        "record":{"name":"Plot","id":"urn:terrain:plot","fields":fields},"choices":[{"name":"Mark","id":"urn:terrain:mark"}]});
    let document = |rust: &str, private: bool| -> Result {
        fs::write(&source, rust)?;
        let mut args = vec![
            "rustdoc",
            "--offline",
            "--lib",
            "--",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
        ];
        if private {
            args.push("--document-private-items");
        }
        command(
            Command::new("cargo")
                .env("RUSTC_BOOTSTRAP", "1")
                .env("CARGO_TARGET_DIR", &target),
            root,
            &args,
            None,
        )?;
        Ok(())
    };
    let recover = |label: &str, error: Option<&str>, overrides: Value| -> Result {
        let mut spec = selection.clone();
        spec["sourceSha256"] = json!(sha(&source)?);
        spec["rustdocSha256"] = json!(sha(&target.join("doc/audit_plot.json"))?);
        spec.as_object_mut()
            .unwrap()
            .extend(overrides.as_object().unwrap().clone());
        write(&root.join("selection.json"), &spec)?;
        invoke(
            &cli,
            root,
            &[
                "recover",
                "serde-public-record",
                "src/model.rs",
                "doc-target/doc/audit_plot.json",
                "selection.json",
            ],
            label,
            error,
        )
    };
    let native = |label: &str, policy: &str, order: &str| -> Result {
        let model = root.join(label);
        invoke(
            &cli,
            &model,
            &["build", "public-records", "rust", "graph.json", "rust.json"],
            "projected",
            None,
        )?;
        let original = fs::read(&source)?;
        let args = ["run", "--offline", "--", policy, order];
        cargo(root, &args)?;
        let enum_source = SOURCE
            .split_once(&format!("{DERIVE}\npub struct"))
            .unwrap()
            .0;
        fs::write(
            &source,
            format!(
                "{enum_source}{}",
                fs::read_to_string(model.join("projected/records.rs"))?
            ),
        )?;
        cargo(root, &args)?;
        fs::write(&source, original)?;
        Ok(())
    };
    document(SOURCE, true)?;
    recover("baseline", None, json!({}))?;
    let mut baseline = read(&root.join("baseline/graph.json"))?;
    let binding = read(&root.join("baseline/rust.json"))?;
    let record = &baseline["modules"][0]["records"][0];
    assert_eq!(
        record["traits"],
        json!([
            "clone",
            "debug",
            "partial-eq",
            "eq",
            "serialize",
            "deserialize"
        ])
    );
    let fields = record["fields"].as_array().unwrap();
    assert_eq!(
        json!(fields.iter().map(|f| &f["wire"]).collect::<Vec<_>>()),
        json!(["from", "until", "mark", "fixed"])
    );
    assert_eq!(
        json!(fields.iter().map(|f| &f["type"]).collect::<Vec<_>>()),
        json!(["u32",{"optional":"u32"},{"optional":{"external":"urn:terrain:mark"}},"bool"])
    );
    assert_eq!(
        binding["externalTypes"],
        json!({"urn:terrain:mark":["crate","Mark"]})
    );
    assert_eq!(
        record["readPolicy"],
        json!({"shape":"object-or-ordered-sequence","unknownFields":"ignore"})
    );
    recover(
        "wrong-type",
        Some("field declaration"),
        json!({"choices":[{"id":"urn:terrain:mark","name":"Plot"}]}),
    )?;
    for (label, key) in [("stale", "sourceSha256"), ("stale-root", "crateRootSha256")] {
        recover(label, Some("digest"), json!({key:"0".repeat(64)}))?;
    }
    native("baseline", "open", "initial")?;
    document(
        &SOURCE.replace(
            "pub struct Plot",
            "#[serde(deny_unknown_fields)]\npub struct Plot",
        ),
        true,
    )?;
    recover("mutated", None, json!({}))?;
    baseline["modules"][0]["records"][0]["readPolicy"]["unknownFields"] = json!("reject");
    assert_eq!(read(&root.join("mutated/graph.json"))?, baseline);
    assert_eq!(read(&root.join("mutated/rust.json"))?, binding);
    native("mutated", "closed", "initial")?;
    document(
        &SOURCE.replace(
            "    pub from: u32,\n    pub until: Option<u32>,",
            "    pub until: Option<u32>,\n    pub from: u32,",
        ),
        true,
    )?;
    recover("reordered", None, json!({}))?;
    let record = &mut baseline["modules"][0]["records"][0];
    record["readPolicy"]["unknownFields"] = json!("ignore");
    record["fields"].as_array_mut().unwrap().swap(0, 1);
    assert_eq!(read(&root.join("reordered/graph.json"))?, baseline);
    assert_eq!(read(&root.join("reordered/rust.json"))?, binding);
    native("reordered", "open", "reordered")?;
    document(
        &SOURCE.replace(
            "pub struct Plot",
            "#[serde(rename_all = \"camelCase\")]\npub struct Plot",
        ),
        true,
    )?;
    recover(
        "unsupported-policy",
        Some("unsupported Serde record attribute"),
        json!({}),
    )?;
    document(
        &SOURCE.replace(
            &format!("{DERIVE}\npub struct Plot"),
            &format!("pub type Option<T> = Vec<T>;\n\n{DERIVE}\npub struct Plot"),
        ),
        true,
    )?;
    recover("shadowed-option", Some("shadowed type"), json!({}))?;
    for (label, crate_root, private, error) in [
        (
            "missing-root-export",
            "mod model;\npub use model::Plot;\n",
            true,
            "expected one crate-root export of Mark",
        ),
        (
            "private-root-record",
            "mod model;\npub use model::*;\nstruct Plot;\n",
            true,
            "crate-root export",
        ),
        (
            "private-root-choice",
            "mod model;\npub use model::*;\nmod other { pub struct Mark; }\nuse other::Mark;\n",
            true,
            "crate-root export",
        ),
        (
            "public-docs-omit-shadow",
            "pub mod model;\npub use model::*;\nstruct Plot;\n",
            false,
            "must include private items",
        ),
    ] {
        fs::write(&lib, crate_root)?;
        document(SOURCE, private)?;
        recover(label, Some(error), json!({"crateRootSha256":sha(&lib)?}))?;
    }
    println!(
        "Recovered default and strict Serde policy, source field order, and compiler-checked choice links"
    );
    Ok(())
}
