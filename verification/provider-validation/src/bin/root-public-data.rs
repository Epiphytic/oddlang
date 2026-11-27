//! Compile recovered root records and errors and reject fifteen unsupported source declarations.
use provider_validation::{Result, cli, command, invoke, sha, write};
use serde::Deserialize;
use serde_json::json;
use std::{fs, path::Path, process::Command};
fn recover(
    cli: &Path,
    root: &Path,
    source: &str,
    profile: &str,
    out: &str,
    rejection: bool,
) -> Result {
    let path = root.join("lib.rs");
    fs::write(&path, source)?;
    let mut selection = json!({"sourceSha256":sha(&path)?,"id":"urn:sensor:public"});
    match profile {
        "finite-error-domain" => {
            selection["format"] = "oddlang-finite-error-selection-v1".into();
            selection["error"] = "Fault".into();
        }
        "public-root-records" => {
            selection["format"] = "oddlang-rust-root-record-selection-v1".into();
            selection["records"] = json!({"Snapshot":"urn:sensor:snapshot"});
        }
        _ => return Err(format!("unexpected fixture profile {profile}").into()),
    }
    write(&root.join("selection.json"), &selection)?;
    invoke(
        cli,
        root,
        &["recover", profile, "lib.rs", "selection.json"],
        out,
        rejection.then_some(""),
    )
}
#[derive(Deserialize)]
struct Rejection {
    command: String,
    name: String,
    source: String,
}
fn main() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let source = include_str!("../../fixtures/root-public-data/source.rs");
    for (profile, target, file) in [
        ("finite-error-domain", "finite-error-domain", "errors.rs"),
        ("public-root-records", "public-records", "records.rs"),
    ] {
        recover(&cli, root, source, profile, profile, false)?;
        let projection = format!("{profile}-projected");
        invoke(
            &cli,
            root,
            &[
                "build",
                target,
                "rust",
                &format!("{profile}/graph.json"),
                &format!("{profile}/rust.json"),
            ],
            &projection,
            None,
        )?;
        fs::copy(root.join(projection).join(file), root.join(file))?;
    }
    fs::write(
        root.join("main.rs"),
        include_bytes!("../../fixtures/root-public-data/caller.rs"),
    )?;
    command(
        &mut Command::new("rustc"),
        root,
        &["--edition=2021", "main.rs", "-o", "probe"],
        None,
    )?;
    command(&mut Command::new(root.join("probe")), root, &[], None)?;
    let cases: Vec<Rejection> = serde_json::from_str(include_str!(
        "../../fixtures/root-public-data/rejections.json"
    ))?;
    assert_eq!(cases.len(), 15);
    for case in cases {
        recover(&cli, root, &case.source, &case.command, &case.name, true)?;
    }
    Ok(())
}
