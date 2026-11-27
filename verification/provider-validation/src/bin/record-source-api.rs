//! Qualify Rust source requirements against an unchanged native caller.
use provider_validation::{Result, cargo, cli, invoke, write};
use serde_json::{Value, json};
use std::fs;

const FIELD: &str = "urn:calendar:appointment:until";
const RECORD: &str = "urn:calendar:appointment";

fn main() -> Result {
    let cli = cli()?;
    let work = tempfile::tempdir()?;
    let root = work.path();
    fs::write(
        root.join("graph.json"),
        include_bytes!("../../fixtures/public-records/source_api/graph.json"),
    )?;
    let binding: Value = serde_json::from_str(include_str!(
        "../../fixtures/public-records/source_api/binding.json"
    ))?;
    let api: Value = serde_json::from_str(include_str!(
        "../../fixtures/public-records/source_api/api.json"
    ))?;
    let build = |label, error| {
        invoke(
            &cli,
            root,
            &["build", "public-records", "rust", "graph.json", "rust.json"],
            label,
            error,
        )
    };
    let project = |label, requirement: Option<Value>, error| -> Result {
        let mut binding = binding.clone();
        if let Some(api) = requirement {
            binding["sourceApi"] = api;
        }
        write(&root.join("rust.json"), &binding)?;
        build(label, error)
    };
    project("legacy", None, None)?;
    project("checked", Some(api.clone()), None)?;
    assert_eq!(
        fs::read(root.join("legacy/records.rs"))?,
        fs::read(root.join("checked/records.rs"))?
    );
    for (label, key, value, error) in [
        (
            "field-name",
            "name",
            json!("ending"),
            "source API field name differs",
        ),
        (
            "field-type",
            "type",
            json!({"optional":"u32"}),
            "source API field type differs",
        ),
        (
            "field-visibility",
            "visibility",
            json!("private"),
            "unknown variant",
        ),
    ] {
        let mut changed = api.clone();
        changed["fields"][FIELD][key] = value;
        project(label, Some(changed), Some(error))?;
    }
    for (label, key, value, error) in [
        ("wrong-role", "role", json!("wire"), "unknown variant"),
        (
            "unknown-field",
            "fields",
            json!({RECORD:api["fields"][FIELD]}),
            "unknown source API field",
        ),
        (
            "empty",
            "fields",
            json!({}),
            "source API field requirements are empty",
        ),
    ] {
        let mut changed = api.clone();
        changed[key] = value;
        project(label, Some(changed), Some(error))?;
    }
    let mut duplicate = binding;
    duplicate["sourceApi"] = api.clone();
    let entry = format!("{}:{}", serde_json::to_string(FIELD)?, api["fields"][FIELD]);
    fs::write(
        root.join("rust.json"),
        duplicate
            .to_string()
            .replacen(&entry, &format!("{entry},{entry}"), 1),
    )?;
    build("duplicate", Some("duplicate binding key"))?;

    fs::create_dir(root.join("src"))?;
    fs::copy(root.join("checked/records.rs"), root.join("src/lib.rs"))?;
    fs::write(
        root.join("src/main.rs"),
        include_bytes!("../../fixtures/public-records/source_api/caller.rs"),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='calendar_api'\nversion='0.0.0'\nedition='2024'\n[dependencies]\nserde={version='=1.0.229',features=['derive']}\nserde_json='=1.0.154'\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    cargo(root, &["run", "--offline", "--locked"])?;
    println!(
        "Checked source API caller, portable optional-u64 bounds and seven rejection controls"
    );
    Ok(())
}
