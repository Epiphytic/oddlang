mod linkage;
#[path = "../schema_support/mod.rs"]
mod schema_support;
use provider_validation::{Result, command, read};
use schema_support::{Suite, cargo};
use std::{collections::BTreeSet, fs, process::Command};

fn telemetry(s: &Suite) -> Result {
    let model = s.json("telemetry.json")?;
    s.project(&model, "projected", None)?;
    let projected = s.root.join("projected");
    let native = s.case("telemetry-oracle.rs");
    fs::copy(&native, projected.join("main.rs"))?;
    command(
        &mut Command::new("rustc"),
        &projected,
        &["--edition=2024", "main.rs", "-o", "witness"],
        None,
    )?;
    command(
        &mut Command::new(projected.join("witness")),
        &projected,
        &[],
        None,
    )?;
    let catalog = read(&projected.join("obligations.json"))?;
    let actual: BTreeSet<_> = catalog["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["declaration"].as_str().unwrap())
        .collect();
    let mut expected: BTreeSet<_> = model["graph"]["layout"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    expected.insert(model["graph"]["id"].as_str().unwrap());
    assert_eq!(actual, expected);
    s.rejections(&model, "telemetry-rejections.json")?;
    fs::create_dir(s.root.join("src"))?;
    fs::write(s.root.join("src/lib.rs"), "")?;
    fs::copy(s.case("telemetry-Cargo.toml"), s.root.join("Cargo.toml"))?;
    let mut manifest = s.json("telemetry-package.json")?;
    manifest["components"][1]["graph"] = model["graph"].clone();
    manifest["components"][1]["overlay"] = model["binding"].clone();
    linkage::qualify(s, manifest, "telemetry-linkage.json")?;
    let package = s.root.join("package");
    fs::create_dir(package.join("examples"))?;
    let source = fs::read_to_string(native)?;
    let body = source
        .split_once("fn main()")
        .ok_or("missing telemetry witness")?
        .1;
    fs::write(
        package.join("examples/witness.rs"),
        format!("use telemetry_fixture::{{Error,Mode,Sample}};\nfn main(){body}"),
    )?;
    cargo(
        &package,
        &["run", "--locked", "--offline", "--example", "witness"],
    )?;
    for name in ["nested-valid", "same-error-reexport", "three-level-valid"] {
        cargo(&s.root.join(name), &["check", "--locked", "--offline"])?;
    }
    Ok(())
}
fn direct(s: &Suite, name: &str) -> Result {
    let model = s.json(&format!("{name}.json"))?;
    if name == "composite" {
        s.project(&s.json("composite-inner.json")?, "inner", None)?;
    }
    s.project(&model, "src", None)?;
    if name == "composite" {
        fs::copy(s.root.join("inner/record.rs"), s.root.join("src/inner.rs"))?;
    }
    fs::copy(
        s.case(&format!("{name}-oracle.rs")),
        s.root.join("src/main.rs"),
    )?;
    fs::copy(
        s.case(&format!("{name}-Cargo.toml")),
        s.root.join("Cargo.toml"),
    )?;
    cargo(&s.root, &["generate-lockfile", "--offline"])?;
    cargo(&s.root, &["run", "--locked", "--offline"])?;
    s.rejections(&model, &format!("{name}-rejections.json"))
}
fn main() -> Result {
    let (name, s) = Suite::open("binary-record")?;
    match name.as_str() {
        "telemetry" => telemetry(&s)?,
        "package" => linkage::geology(&s)?,
        "metadata" | "composite" => direct(&s, &name)?,
        _ => return Err("expected telemetry|package|metadata|composite ODDLANG".into()),
    }
    println!("Binary-record {name} qualification passed");
    Ok(())
}
