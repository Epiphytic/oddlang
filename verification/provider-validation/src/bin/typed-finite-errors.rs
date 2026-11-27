//! Recover typed error variants and qualify their native package linkage.
use provider_validation::{Result, cargo, cli, command, invoke, pin, sha, write};
use serde_json::json;
use std::{fs, path::Path, process::Command};
const SOURCE: &str = include_str!("../../../typed-finite-errors/source.rs");
fn recover(cli: &Path, root: &Path, source: &str, reason: Option<&str>) -> Result {
    fs::create_dir(root)?;
    let input = root.join("selected.rs");
    fs::write(&input, source)?;
    write(
        &root.join("selection.json"),
        &json!({
            "format":"oddlang-typed-finite-error-selection-v1","id":"urn:example:archive","sourceSha256":sha(&input)?,
            "error":"ArchiveFault","result":"Outcome","choices":["DocumentReason"],"variantIds":{"Unavailable":"urn:example:availability"}
        }),
    )?;
    invoke(
        cli,
        root,
        &[
            "recover",
            "finite-error-domain",
            "selected.rs",
            "selection.json",
        ],
        "recovered",
        reason,
    )
}
fn process_link(cli: &Path, root: &Path) -> Result {
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"typed-archive\"\nversion = \"0.1.0\"\n",
    )?;
    let failure = "urn:example:availability";
    let errors = json!({"kind":"finite-error-domain","module":"types","reexports":["ArchiveFault","Outcome","DocumentReason"],
        "graph":pin(root,"recovered/graph.json")?,"overlay":pin(root,"recovered/rust.json")?});
    let mut selector = json!({"entry":"initialize_budget","error":{"id":failure,"typePath":["ArchiveFault"],"variant":"Unavailable"}});
    let policy = json!({"format":"oddlang-process-limit-v1","id":"urn:example:policy","failure":failure,"resource":"open-descriptors","platforms":{"linux":{"kind":"no-op"},"macos":{"kind":"no-op"}}});
    write(&root.join("policy.json"), &policy)?;
    write(&root.join("policy-rust.json"), &selector)?;
    let process = json!({"kind":"process-limit","module":"limits","reexports":["initialize_budget"],"graph":pin(root,"policy.json")?,"overlay":pin(root,"policy-rust.json")?});
    let package = json!({"format":"oddlang-native-package-v1","name":"typed-archive","version":"0.1.0","edition":"2021",
        "cargoLock":pin(root,"Cargo.lock")?,"dependencies":{},"components":[errors,process]});
    write(&root.join("package.json"), &package)?;
    let args = ["build", "native-package", "rust", "package.json"];
    invoke(cli, root, &args, "product", None)?;
    let product = root.join("product");
    fs::create_dir(product.join("src/bin"))?;
    fs::write(
        product.join("src/bin/test.rs"),
        include_str!("../../../typed-finite-errors/linked.rs"),
    )?;
    cargo(&product, &["run", "--locked", "--offline", "--bin", "test"])?;
    for (name, variant, mut components) in [
        ("absent", "Unavailable", vec![process.clone()]),
        ("misbound", "Rejected", vec![errors, process]),
    ] {
        let mut negative = package.clone();
        selector["error"]["variant"] = json!(variant);
        write(&root.join("negative-rust.json"), &selector)?;
        components.last_mut().ok_or("missing process")?["overlay"] =
            pin(root, "negative-rust.json")?;
        negative["components"] = json!(components);
        write(&root.join("package.json"), &negative)?;
        invoke(
            cli,
            root,
            &args,
            name,
            Some("unresolved declared process limit error"),
        )?;
    }
    Ok(())
}
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    for (name, message) in [
        ("baseline", "archive unavailable"),
        ("changed", "archive closed"),
    ] {
        let work = root.join(name);
        recover(
            &cli,
            &work,
            &SOURCE.replace("archive unavailable", message),
            None,
        )?;
        invoke(
            &cli,
            &work,
            &[
                "build",
                "finite-error-domain",
                "rust",
                "recovered/graph.json",
                "recovered/rust.json",
            ],
            "projection",
            None,
        )?;
        fs::write(
            work.join("probe.rs"),
            include_str!("../../../typed-finite-errors/probe.rs"),
        )?;
        command(
            &mut Command::new("rustc"),
            &work,
            &["--edition=2021", "probe.rs"],
            None,
        )?;
        assert_eq!(
            command(&mut Command::new(work.join("probe")), &work, &[], None)?,
            format!("{message}\n").as_bytes()
        );
    }
    process_link(&cli, &root.join("baseline"))?;
    recover(
        &cli,
        &root.join("unsupported"),
        &SOURCE.replace("capacity: u32", "capacity: String"),
        Some("unsupported typed error payload field"),
    )?;
    for (i, shadow) in [
        "pub enum u32 { Case }",
        "pub union u32 { byte: u8 }",
        "pub trait u32 {}",
        "pub enum Debug { Case }",
    ]
    .iter()
    .enumerate()
    {
        recover(
            &cli,
            &root.join(format!("shadow-{i}")),
            &format!("{SOURCE}\n{shadow}"),
            Some("shadowing typed error declaration"),
        )?;
    }
    println!("Unrelated typed enum, fixed-width product and diagnostic mutation passed.");
    Ok(())
}
