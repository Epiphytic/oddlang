//! Qualify a renamed record reader with restricted capacity and its pinned native providers.
use provider_validation::{Result, cargo, cli, invoke, pin, read, write};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path};
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/bounded-json-input");
fn qualify(root: &Path, product: &str) -> Result {
    let product = root.join(product);
    fs::OpenOptions::new()
        .append(true)
        .open(product.join("src/group_1/__projection_1.rs"))?
        .write_all(include_bytes!(
            "../../fixtures/bounded-json-input/consumer.rs"
        ))?;
    let log = cargo(&product, &["test", "--locked", "--offline"])?;
    if !String::from_utf8(log)?.contains("1 passed") {
        return Err("reader consumer did not run".into());
    }
    Ok(())
}
fn project(
    cli: &Path,
    root: &Path,
    manifest: &Value,
    name: &str,
    expected: Option<&str>,
) -> Result {
    write(&root.join("package.json"), manifest)?;
    invoke(
        cli,
        root,
        &["build", "native-package", "rust", "package.json"],
        name,
        expected,
    )
}
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='small-observer'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nserde_json='=1.0.151'\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    for file in [
        "graph.json",
        "binding.json",
        "error.json",
        "error-rust.json",
    ] {
        fs::copy(Path::new(FIXTURE).join(file), root.join(file))?;
    }
    let dependency = json!({"serde_json":{"version":"1.0.151"}});
    let mut manifest = json!({"format":"oddlang-native-package-v1","name":"small-observer","version":"0.1.0",
        "edition":"2021","dependencies":dependency,"cargoLock":pin(root,"Cargo.lock")?,
        "components":[{"kind":"finite-error-domain","module":"faults","reexports":["Fault"],
            "graph":pin(root,"error.json")?,"overlay":pin(root,"error-rust.json")?},
            {"kind":"bounded-json-input","module":"metadata","reexports":[],"graph":pin(root,"graph.json")?,"overlay":pin(root,"binding.json")?}]});
    project(&cli, root, &manifest, "product", None)?;
    qualify(root, "product")?;
    manifest.as_object_mut().unwrap().remove("dependencies");
    manifest["devDependencies"] = dependency.clone();
    project(&cli, root, &manifest, "test-product", None)?;
    let dev = root.join("test-product");
    let tree = cargo(&dev, &["tree", "--locked", "--offline", "-e", "normal"])?;
    if String::from_utf8(tree)?.contains("serde_json") {
        return Err("dev-only provider leaked into normal dependencies".into());
    }
    cargo(&dev, &["build", "--release", "--locked", "--offline"])?;
    qualify(root, "test-product")?;
    manifest["dependencies"] = dependency.clone();
    project(
        &cli,
        root,
        &manifest,
        "duplicate-dependency",
        Some("normal and dev dependencies must have distinct names"),
    )?;
    manifest.as_object_mut().unwrap().remove("dependencies");
    manifest["devDependencies"]["serde_json"]["defaultFeatures"] = false.into();
    project(
        &cli,
        root,
        &manifest,
        "no-defaults",
        Some("default locked serde_json provider"),
    )?;
    manifest["devDependencies"] = dependency;
    let mut binding = read(&root.join("binding.json"))?;
    binding["variants"]["urn:test:capacity"] = "Unknown".into();
    write(&root.join("binding.json"), &binding)?;
    manifest["components"][1]["overlay"] = pin(root, "binding.json")?;
    project(
        &cli,
        root,
        &manifest,
        "unbound",
        Some("unresolved bounded JSON unit error"),
    )
}
