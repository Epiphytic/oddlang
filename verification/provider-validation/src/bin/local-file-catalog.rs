//! Qualify inert catalog parsing and the links between its manifest, file and error contracts.
use provider_validation::{Result, cargo, cli, invoke, pin, read, write};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path};
fn prepare(root: &Path, name: &str, dependencies: &str) -> Result {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n[dependencies]\n{dependencies}\n"
        ),
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    Ok(())
}
fn standalone(cli: &Path, fixture: &Path, root: &Path) -> Result {
    prepare(root, "catalog-observer", "sha2='=0.10.9'")?;
    let graph = read(&fixture.join("inert.json"))?;
    write(&root.join("graph.json"), &graph)?;
    fs::copy(fixture.join("inert-rust.json"), root.join("rust.json"))?;
    let args = [
        "build",
        "local-file-catalog",
        "rust",
        "graph.json",
        "rust.json",
    ];
    invoke(cli, root, &args, "generated", None)?;
    fs::copy(
        root.join("generated/catalog.rs"),
        root.join("src/catalog.rs"),
    )?;
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../local-file-catalog/caller.rs"),
        root.join("src/lib.rs"),
    )?;
    cargo(
        root,
        &["test", "--locked", "--offline", "--", "--nocapture"],
    )?;
    for (field, value) in [
        ("fixedCatalog", json!("../outside")),
        ("suffix", json!("/metadata")),
        ("prefixes", json!(["v", "v1"])),
        ("assetNames", json!(["extra", "../record"])),
        ("guards", json!(["parse", "catalog-digest"])),
        ("candidateNameEncoding", json!("raw-bytes")),
        ("attributeChecks", json!("both")),
    ] {
        let mut bad = graph.clone();
        bad[field] = value;
        write(&root.join("graph.json"), &bad)?;
        invoke(
            cli,
            root,
            &args,
            "refused",
            Some("unsupported local file catalog policy"),
        )?;
    }
    Ok(())
}
fn assembly(cli: &Path, fixture: &Path, root: &Path) -> Result {
    prepare(root, "catalog-links", "libc='=0.2.189'\nsha2='=0.10.9'")?;
    let mut components = Vec::new();
    for (module, kind, public, inner) in [
        (
            "fault",
            "finite-error-domain",
            vec!["Fault", "Result"],
            vec![],
        ),
        ("formats", "line-grammar", vec![], vec!["Catalog"]),
        ("reads", "read-only-file", vec![], vec!["bounded", "verify"]),
        (
            "inert",
            "local-file-catalog",
            vec![],
            vec!["Naming", "inspect_catalog"],
        ),
    ] {
        let graph = format!("{module}.json");
        let overlay = format!("{module}-rust.json");
        for file in [&graph, &overlay] {
            fs::copy(fixture.join(file), root.join(file))?;
        }
        components.push(json!({"kind":kind,"module":module,"reexports":public,"crateReexports":inner,"graph":pin(root,&graph)?,"overlay":pin(root,&overlay)?}));
    }
    let manifest = json!({"format":"oddlang-native-package-v1","name":"catalog-links","version":"0.1.0","edition":"2021","cargoLock":pin(root,"Cargo.lock")?,
        "dependencies":{"libc":{"version":"0.2.189"},"sha2":{"version":"0.10.9"}},"components":components});
    let file = root.join("package.json");
    let args = ["build", "native-package", "rust", "package.json"];
    write(&file, &manifest)?;
    invoke(cli, root, &args, "product", None)?;
    let product = root.join("product");
    cargo(&product, &["check", "--offline", "--locked"])?;
    fs::OpenOptions::new()
        .append(true)
        .open(product.join("src/lib.rs"))?
        .write_all(include_bytes!(
            "../../fixtures/local-file-catalog/linked.rs"
        ))?;
    let test = cargo(
        &product,
        &[
            "test",
            "--offline",
            "--locked",
            "--lib",
            "--",
            "--exact",
            "actual_linked_catalog",
        ],
    )?;
    if !String::from_utf8(test)?.contains("1 passed") {
        return Err("linked native catalog caller did not run".into());
    }
    let catalog = read(&fixture.join("inert.json"))?;
    let mut bad_id = catalog.clone();
    bad_id["manifestGrammar"] = "urn:test:wrong".into();
    let mut binding = read(&fixture.join("inert-rust.json"))?;
    binding["invalidVariant"] = "Wrong".into();
    let mut width = read(&fixture.join("formats.json"))?;
    for row in width["rows"].as_array_mut().unwrap().iter_mut().skip(1) {
        row[1]["bits"] = 16.into();
    }
    width["recordResult"]["records"]["asset"][1]["type"]["bits"] = 16.into();
    let mut order = catalog;
    order["assetNames"].as_array_mut().unwrap().reverse();
    let reject = |bad: &Value, reason: &str| -> Result {
        write(&file, bad)?;
        invoke(cli, root, &args, "denied", Some(reason))
    };
    for (i, field, model, reason) in [
        (3, "graph", bad_id, "grammar or error role differs"),
        (3, "overlay", binding, "grammar or error role differs"),
        (1, "graph", width, "asset field types differ"),
        (3, "graph", order, "asset names differ from the grammar"),
    ] {
        let mut bad = manifest.clone();
        write(&root.join("bad.json"), &model)?;
        bad["components"][i][field] = pin(root, "bad.json")?;
        reject(&bad, reason)?;
    }
    let mut public = manifest.clone();
    public["components"][3]["visibility"] = "public".into();
    reject(&public, "needs private crate-scoped bindings")?;
    let mut duplicate = manifest.clone();
    duplicate["components"]
        .as_array_mut()
        .unwrap()
        .push(manifest["components"][2].clone());
    reject(&duplicate, "file policy is ambiguous")
}
fn main() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/local-file-catalog");
    standalone(&cli, &fixture, &tmp.path().join("standalone"))?;
    assembly(&cli, &fixture, &tmp.path().join("assembly"))
}
