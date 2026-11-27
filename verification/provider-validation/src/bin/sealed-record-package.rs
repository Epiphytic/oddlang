//! Compare explicit, compact and mixed manifests after removing the original sources.
use provider_validation::{Result, cargo, cli, command, invoke, pin, read, sha, write};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

fn files(root: &Path, relative: &Path, inventory: &mut BTreeMap<PathBuf, String>) -> Result {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            files(root, &path, inventory)?;
        } else if path != Path::new("package.lock") {
            inventory.insert(path, sha(&entry.path())?);
        }
    }
    Ok(())
}
fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut map = BTreeMap::new();
    files(root, Path::new(""), &mut map)?;
    Ok(map)
}
fn main() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let inputs = root.join("inputs");
    fs::create_dir(&inputs)?;
    let seed = root.join("seed");
    fs::create_dir_all(seed.join("src"))?;
    fs::write(
        seed.join("src/lib.rs"),
        "mod weather_model;\npub use weather_model::*;\n",
    )?;
    let source = seed.join("src/weather_model.rs");
    fs::write(
        &source,
        include_str!("../../../recovered-public-data/weather.rs"),
    )?;
    fs::write(
        seed.join("Cargo.toml"),
        "[package]\nname='weather'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nserde={version='=1.0.229', features=['derive']}\n",
    )?;
    cargo(&seed, &["generate-lockfile", "--offline"])?;
    fs::copy(seed.join("Cargo.lock"), inputs.join("Cargo.lock"))?;
    command(
        Command::new("cargo")
            .env("RUSTC_BOOTSTRAP", "1")
            .env("CARGO_TARGET_DIR", root.join("target")),
        &seed,
        &[
            "rustdoc",
            "--lib",
            "--offline",
            "--locked",
            "--",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
            "--document-private-items",
        ],
        None,
    )?;
    let sky = "urn:weather:sky";
    let forecast = "urn:weather:forecast";
    let source_hash = sha(&source)?;
    write(
        &root.join("choice.json"),
        &json!({"format":"oddlang-rust-serde-string-choice-selection-v1","sourceSha256":source_hash,
        "id":"urn:weather:choice-domain","enumId":sky,"name":"Sky","variants":{"Clear":"urn:weather:sky:clear"}}),
    )?;
    write(
        &root.join("record.json"),
        &json!({"format":"oddlang-serde-public-record-selection-v1","sourceFilename":"src/weather_model.rs",
        "sourceSha256":source_hash,"crateRootSha256":sha(&seed.join("src/lib.rs"))?,"rustdocSha256":sha(&root.join("target/doc/weather.json"))?,
        "crateName":"weather","moduleId":"urn:weather:module","moduleName":"weather_model","record":{"name":"Forecast","id":forecast,"fields":{"sky":"urn:weather:forecast:sky"}},
        "choices":[{"name":"Sky","id":sky}]}),
    )?;
    invoke(
        &cli,
        root,
        &[
            "recover",
            "serde-string-choice",
            "seed/src/weather_model.rs",
            "choice.json",
        ],
        "inputs/choice",
        None,
    )?;
    invoke(
        &cli,
        root,
        &[
            "recover",
            "serde-public-record",
            "seed/src/weather_model.rs",
            "target/doc/weather.json",
            "record.json",
        ],
        "inputs/record",
        None,
    )?;
    fs::remove_dir_all(seed)?;
    let component = |folder: &str, kind: &str, name: &str| -> Result<(Value, Value)> {
        Ok((
            json!({"kind":kind,"module":"weather_model","reexports":[name],"graph":pin(&inputs,&format!("{folder}/graph.json"))?,
            "overlay":pin(&inputs,&format!("{folder}/rust.json"))?}),
            json!({"recovery":pin(&inputs,&format!("{folder}/recovery.lock"))?}),
        ))
    };
    let (choice, compact_choice) = component("choice", "shared-declarations", "Sky")?;
    let (record, compact_record) = component("record", "public-records", "Forecast")?;
    let mut anchored = compact_choice.clone();
    anchored["module"] = json!("weather_model");
    let deps = json!({"serde":{"version":"1.0.229","features":["derive"]}});
    let lock = pin(&inputs, "Cargo.lock")?;
    let build = |label: &str,
                 components: Value,
                 dependencies: &Value,
                 reason: Option<&str>|
     -> Result {
        write(
            &inputs.join("package.json"),
            &json!({"format":"oddlang-native-package-v1","name":"weather","version":"0.1.0","edition":"2024","dependencies":dependencies,
        "cargoLock":lock,"components":components}),
        )?;
        invoke(
            &cli,
            root,
            &["build", "native-package", "rust", "inputs/package.json"],
            label,
            reason,
        )
    };
    build("explicit", json!([choice, record]), &deps, None)?;
    build("compact", json!([anchored, compact_record]), &deps, None)?;
    build("mixed", json!([choice, compact_record]), &deps, None)?;
    let expect = inventory(&root.join("explicit"))?;
    let mut old = read(&root.join("explicit/package.lock"))?;
    old.as_object_mut()
        .unwrap()
        .remove("manifestSha256")
        .unwrap();
    for (name, pins) in [
        ("compact", [Some(&anchored), Some(&compact_record)]),
        ("mixed", [None, Some(&compact_record)]),
    ] {
        let output = root.join(name);
        assert_eq!(inventory(&output)?, expect);
        let mut lock = read(&output.join("package.lock"))?;
        lock.as_object_mut()
            .unwrap()
            .remove("manifestSha256")
            .unwrap();
        let entries = lock["components"].as_array_mut().unwrap();
        assert_eq!(entries.len(), 2);
        for ((row, pin), id) in entries.iter_mut().zip(pins).zip([sky, forecast]) {
            let fields = row.as_object_mut().unwrap();
            assert_eq!(
                fields.remove("recoverySha256"),
                pin.map(|p| p["recovery"]["sha256"].clone())
            );
            assert_eq!(
                fields.remove("recoveredDeclaration"),
                pin.map(|_| json!(id))
            );
        }
        assert_eq!(lock, old);
    }
    let mut moved = compact_record.clone();
    moved["module"] = json!("other");
    let mut both = record.clone();
    both["recovery"] = compact_record["recovery"].clone();
    let mut stale = compact_record.clone();
    stale["recovery"]["sha256"] = json!("0".repeat(64));
    build(
        "missing-serde",
        json!([anchored]),
        &json!({}),
        Some("requires native serde with derive"),
    )?;
    for (name, components, error) in [
        (
            "missing-placement",
            json!([compact_choice, compact_record]),
            "explicit module",
        ),
        (
            "placement-override",
            json!([anchored, moved]),
            "already determines",
        ),
        (
            "duplicate-provider",
            json!([anchored, compact_record, compact_record]),
            "ambiguous recovered public declaration",
        ),
        (
            "duplicate-export",
            json!([anchored, compact_record, record]),
            "conflicting reexport",
        ),
        (
            "missing-provider",
            json!([compact_record]),
            "unresolved external public record choice",
        ),
        (
            "ambiguous-link",
            json!([anchored, choice, compact_record]),
            "ambiguous external public record choice",
        ),
        ("mixed-entry", json!([anchored, both]), "package manifest"),
        (
            "bad-receipt",
            json!([anchored, stale]),
            "input digest mismatch",
        ),
    ] {
        build(name, components, &deps, Some(error))?;
    }
    fs::OpenOptions::new()
        .append(true)
        .open(inputs.join("record/rust.json"))?
        .write_all(b" ")?;
    build(
        "bad-sibling",
        json!([anchored, compact_record]),
        &deps,
        Some("input digest mismatch"),
    )?;
    cargo(
        &root.join("compact"),
        &["check", "--lib", "--locked", "--offline"],
    )?;
    println!("Compact and explicit public-data packages emit identical native bytes");
    Ok(())
}
