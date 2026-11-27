//! Compare source and graph-only projection with the same independently authored Rust consumer.
use provider_validation::{Result, cargo, cli, invoke, pin, read, sha, write};
use serde_json::{Value, json};
use std::{fs, path::Path};

const SOURCE: &str = include_str!("../../fixtures/public-records/verify/source.rs");
const EXPORT: &str = include_str!("../../fixtures/public-records/verify/export.rs");
const MANIFEST: &str = include_str!("../../fixtures/public-records/verify/Cargo.toml.fixture");
const CONSUMER: &str = include_str!("../../fixtures/public-records/verify/consumer.rs");
const BASELINE:&[u8]=b"{\"label\":\"East\\nDrain\",\"epoch\":18446744073709551615,\"flags\":[0,255],\"active\":false}\n";
const MUTANT:&[u8]=b"{\"epoch\":18446744073709551615,\"label\":\"East\\nDrain\",\"flags\":[0,255],\"active\":false}\n";
fn observed(root: &Path) -> Result<Vec<u8>> {
    cargo(
        root,
        &[
            "run",
            "--example",
            "probe",
            "--locked",
            "--offline",
            "--quiet",
        ],
    )
}

fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let work = temp.path();
    let reference = work.join("reference");
    fs::create_dir_all(reference.join("src"))?;
    let root = reference.join("src/lib.rs");
    let records = reference.join("src/sampling.rs");
    let aliases = reference.join("src/exports.rs");
    let manifest = reference.join("Cargo.toml");
    fs::write(
        &root,
        include_bytes!("../../fixtures/public-records/verify/root.rs"),
    )?;
    fs::write(&manifest, MANIFEST)?;
    fs::write(&records, SOURCE)?;
    fs::write(&aliases, EXPORT)?;
    let mut selection = json!({"format":"oddlang-rust-public-record-selection-v1","libSha256":sha(&root)?,
        "cargoSha256":sha(&manifest)?,"modules":[{"id":"urn:test:irrigation:sampling","name":"sampling","records":{"Reading":"urn:test:irrigation:reading"}},
        {"id":"urn:test:irrigation:exports","name":"exports","aliases":{"Archived":"urn:test:irrigation:archived"}}]});
    let recover =
        |selection: &mut Value, label: &str, error: Option<&str>, repin: bool| -> Result {
            if repin {
                for (i, path) in [&records, &aliases].iter().enumerate() {
                    selection["modules"][i]["sha256"] = json!(sha(path)?);
                }
            }
            write(&work.join("selection.json"), selection)?;
            invoke(
                &cli,
                work,
                &[
                    "recover",
                    "public-records",
                    "reference/src/lib.rs",
                    "selection.json",
                ],
                label,
                error,
            )
        };
    recover(&mut selection, "baseline", None, true)?;
    let baseline = work.join("baseline");
    let graph = read(&baseline.join("graph.json"))?;
    let shape = &graph["modules"][0]["records"][0];
    let fields: Vec<_> = shape["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| json!([f["wire"], f["type"]]))
        .collect();
    assert_eq!(
        json!(fields),
        json!([
            ["label", "text"],
            ["epoch", "u64"],
            ["flags", "octets"],
            ["active", "bool"]
        ])
    );
    assert_eq!(
        graph["modules"][1]["aliases"],
        json!([{"id":"urn:test:irrigation:archived","target":"urn:test:irrigation:reading"}])
    );
    assert!(!graph.to_string().contains("must not enter"));
    let lock = fs::read_to_string(baseline.join("recovery.lock"))?;
    for path in [&records, &aliases] {
        assert!(lock.contains(&sha(path)?));
    }
    for file in fs::read_dir(&baseline)? {
        assert!(file?.path().extension().is_none_or(|ext| ext != "rs"));
    }
    fs::create_dir(reference.join("examples"))?;
    fs::write(reference.join("examples/probe.rs"), CONSUMER)?;
    cargo(&reference, &["generate-lockfile", "--offline"])?;
    fs::copy(reference.join("Cargo.lock"), work.join("Cargo.lock"))?;
    assert_eq!(observed(&reference)?, BASELINE);
    fs::write(
        &records,
        SOURCE.replace(
            "    pub label: String,\n    pub epoch: u64,",
            "    pub epoch: u64,\n    pub label: String,",
        ),
    )?;
    recover(&mut selection, "stale", Some("source digest"), false)?;
    recover(&mut selection, "mutated", None, true)?;
    assert_eq!(observed(&reference)?, MUTANT);
    assert_ne!(
        fs::read(baseline.join("graph.json"))?,
        fs::read(work.join("mutated/graph.json"))?
    );

    for (label, source, error) in [
        (
            "conditional",
            SOURCE.replace("#[derive", "#[cfg(feature = \"optional\")]\n#[derive"),
            "unsupported record attribute",
        ),
        (
            "unknown-type",
            SOURCE.replace("Vec<u8>", "Vec<u16>"),
            "unsupported public record field type",
        ),
    ] {
        fs::write(&records, source)?;
        recover(&mut selection, label, Some(error), true)?;
    }
    for (label, statement) in [
        ("shadowed-type", "use crate::CustomString as String;"),
        ("raw-import", "use crate::other::r#String;"),
        ("raw-declaration", "pub struct r#String;"),
        ("grouped-self", "use crate::String::{self};"),
        ("renamed-self", "use crate::Other::{self as String};"),
    ] {
        fs::write(&records, format!("{statement}\n{SOURCE}"))?;
        recover(
            &mut selection,
            label,
            Some("shadowed public record type"),
            true,
        )?;
    }
    fs::write(&records, SOURCE)?;
    selection["modules"][1]["aliases"]["Archived"] = json!("urn:test:irrigation:reading");
    recover(
        &mut selection,
        "duplicate-id",
        Some("duplicate public record identity"),
        true,
    )?;
    selection["modules"][1]["aliases"]["Archived"] = json!("urn:test:irrigation:archived");
    fs::write(&manifest, MANIFEST.replace("\"2021\"", "\"2015\""))?;
    recover(
        &mut selection,
        "stale-cargo-digest",
        Some("Cargo manifest digest"),
        true,
    )?;
    selection["cargoSha256"] = json!(sha(&manifest)?);
    recover(
        &mut selection,
        "legacy-edition",
        Some("requires an explicit 2018 or newer"),
        true,
    )?;
    fs::write(
        &manifest,
        format!("{MANIFEST}\n[lib]\nedition = \"2015\"\n"),
    )?;
    selection["cargoSha256"] = json!(sha(&manifest)?);
    recover(
        &mut selection,
        "library-edition-override",
        Some("library edition override"),
        true,
    )?;
    fs::write(&manifest, MANIFEST)?;
    selection["cargoSha256"] = json!(sha(&manifest)?);
    fs::write(&records, format!("use ext::other as derive;\n{SOURCE}"))?;
    recover(
        &mut selection,
        "derivation-shadow",
        Some("shadowed public record type or derive"),
        true,
    )?;
    fs::write(&records, SOURCE)?;
    fs::write(&aliases, "pub use crate::missing::{Reading as Archived};")?;
    recover(
        &mut selection,
        "unresolved-alias",
        Some("unresolved public record re-export"),
        true,
    )?;
    fs::write(&aliases, format!("#![cfg(unix)]\n{EXPORT}"))?;
    recover(
        &mut selection,
        "inner-cfg",
        Some("unsupported inner public record module attribute"),
        true,
    )?;
    fs::write(&aliases, EXPORT)?;
    fs::write(
        &records,
        format!("macro_rules! shadow {{ () => {{ pub struct String; }} }}\nshadow!();\n{SOURCE}"),
    )?;
    recover(
        &mut selection,
        "macro-shadow",
        Some("unsupported expanded item"),
        true,
    )?;
    let scope = reference.join("src/scope.rs");
    fs::write(&scope, "pub(crate) struct Extra;\n")?;
    fs::write(
        &records,
        format!("#[path=\"scope.rs\"] mod scope;\nuse self::scope::*;\n{SOURCE}"),
    )?;
    recover(
        &mut selection,
        "unresolved-glob",
        Some("unpinned public record glob source"),
        true,
    )?;
    selection["modules"][0]["scopeGlobs"] = json!({"scope":sha(&scope)?});
    recover(&mut selection, "pinned-glob", None, true)?;
    assert_eq!(
        fs::read(work.join("pinned-glob/graph.json"))?,
        fs::read(baseline.join("graph.json"))?
    );
    fs::write(&scope, "pub(crate) struct String;\n")?;
    selection["modules"][0]["scopeGlobs"]["scope"] = json!(sha(&scope)?);
    recover(
        &mut selection,
        "glob-shadow",
        Some("shadowed public record type through glob"),
        true,
    )?;
    fs::remove_dir_all(&reference)?;

    for (label, expected) in [("baseline", BASELINE), ("mutated", MUTANT)] {
        let graph = format!("{label}/graph.json");
        let package = json!({"format":"oddlang-native-package-v1","name":"indicator-core","edition":"2021","version":"0.1.0",
            "dependencies":{"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.151"}},
            "cargoLock":pin(work,"Cargo.lock")?,"components":[{"kind":"public-records","module":"record_views","graph":pin(work,&graph)?,
            "overlay":pin(work,&format!("{label}/rust.json"))?,"reexports":["sampling","exports"]}]});
        write(&work.join("package.json"), &package)?;
        let output = format!("project-{label}");
        invoke(
            &cli,
            work,
            &["build", "native-package", "rust", "package.json"],
            &output,
            None,
        )?;
        let product = work.join(output);
        fs::create_dir(product.join("examples"))?;
        fs::write(product.join("examples/probe.rs"), CONSUMER)?;
        assert_eq!(observed(&product)?, expected);
        assert_eq!(
            read(&product.join("package.lock"))?["components"][0]["projection"]["graphSha256"],
            sha(&work.join(graph))?
        );
        assert!(
            !fs::read_to_string(product.join("src/group_0/__projection_0.rs"))?
                .contains("must not enter")
        );
    }
    println!(
        "Source recovery, graph-only projection and exact public record alias identity match baseline and mutation"
    );
    Ok(())
}
