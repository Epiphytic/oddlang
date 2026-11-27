//! Compose independent packages, link a sibling edge, and acquire a pinned dependency.
#[path = "native_workspace/bootstrap.rs"]
mod bootstrap;
mod native_artifacts;
use provider_validation::{Result, cli, command, pin, read, write};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn put(root: &Path, file: &str, value: &Value) -> Result<Value> {
    write(&root.join(file), value)?;
    pin(root, file)
}
fn make(root: &Path, name: &str, constant: &str, value: u32) -> Result<Value> {
    let id = format!("urn:meter:{constant}");
    let graph = put(
        root,
        &format!("{name}.json"),
        &json!({"format":"oddlang-shared-declarations-v1","id":format!("urn:meter:{name}"),"constants":[{"id":id,"value":value}]}),
    )?;
    let overlay = put(
        root,
        &format!("{name}-rust.json"),
        &json!({"format":"oddlang-shared-rust-v1","names":{(&id):constant},"constantTypes":{(&id):"u32"}}),
    )?;
    let lock = format!("{name}.lock");
    fs::write(
        root.join(&lock),
        format!("version = 4\n[[package]]\nname=\"{name}\"\nversion=\"0.1.0\"\n"),
    )?;
    put(
        root,
        &format!("{name}-package.json"),
        &json!({"format":"oddlang-native-package-v1","name":name,"version":"0.1.0","edition":"2024","dependencies":{},
        "cargoLock":pin(root,&lock)?,"components":[{"kind":"shared-declarations","module":"values","graph":graph,"overlay":overlay,"reexports":[constant]}]}),
    )
}
fn main() -> Result {
    let cli = cli()?;
    let scratch = tempfile::tempdir()?;
    let root = scratch.path();
    let thermometer = make(root, "thermometer", "LIMIT", 20)?;
    let station = make(root, "station", "PRESSURE", 90)?;
    fs::write(
        root.join("Cargo.lock"),
        "version=4\n[[package]]\nname='thermometer'\nversion='0.1.0'\n[[package]]\nname='station'\nversion='0.1.0'\ndependencies=['thermometer']\n",
    )?;
    let mut workspace = json!({"format":"oddlang-native-workspace-v1","cargoLock":pin(root,"Cargo.lock")?,
        "members":{"thermometer":{"manifest":thermometer},"station":{"manifest":station,"dependencies":["thermometer"]}}});
    let build = |model: &Value, name: &str, error: Option<&str>| -> Result {
        write(&root.join("workspace.json"), model)?;
        command(
            &mut Command::new(&cli),
            root,
            &["build", "native-workspace", "rust", "workspace.json", name],
            error,
        )?;
        if error.is_some() && name != "generated" {
            assert!(!root.join(name).exists());
        }
        Ok(())
    };
    build(&workspace, "generated", None)?;
    build(&workspace, "repeat", None)?;
    let assembled = root.join("generated");
    assert_eq!(
        native_artifacts::inventory(&assembled)?,
        native_artifacts::inventory(&root.join("repeat"))?
    );
    let cargo_test = || {
        command(
            &mut Command::new("cargo"),
            &assembled,
            &["test", "--workspace", "--locked", "--offline"],
            None,
        )
    };
    cargo_test()?;
    fs::create_dir(assembled.join("station/tests"))?;
    fs::write(
        assembled.join("station/tests/consumer.rs"),
        "#[test] fn permitted_values(){assert_eq!(station::PRESSURE,90);assert_eq!(thermometer::LIMIT,20);}",
    )?;
    cargo_test()?;
    build(&workspace, "generated", Some("already exists"))?;
    for (dependency, reason) in [
        ("station", "self workspace edge"),
        ("unknown", "unresolved"),
    ] {
        workspace["members"]["station"]["dependencies"] = json!([dependency]);
        build(&workspace, "missing", Some(reason))?;
    }
    workspace["members"]["station"]["dependencies"] = json!(["thermometer"]);
    workspace["members"]["thermometer"]["dependencies"] = json!(["station"]);
    build(&workspace, "cycle", Some("cyclic"))?;
    workspace["members"]["thermometer"]
        .as_object_mut()
        .unwrap()
        .remove("dependencies");
    workspace["members"]["station"]["manifest"]["sha256"] = json!("0".repeat(64));
    build(&workspace, "tampered", Some("digest mismatch"))?;
    println!(
        "Two checked packages; sibling consumer, fresh-output, cycle, identity and digest controls passed"
    );
    bootstrap::verify(&cli, &root.join("bootstrap"))?;
    println!(
        "Empty-cache failure, pinned Git acquisition, disconnected rebuild and stale root lock checked"
    );
    Ok(())
}
