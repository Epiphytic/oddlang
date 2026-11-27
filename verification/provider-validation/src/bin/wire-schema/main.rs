mod garden;
mod scalar;
#[path = "../schema_support/mod.rs"]
mod schema_support;
use provider_validation::{Result, command, invoke, read, sha, write};
use schema_support::{Suite, cargo};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const OBSERVED: &[&str] = &["graph.json", "rust.json", "observation.json"];
const PROJECTED: &[&str] = &["wire.rs", "obligations.json", "projection.lock"];
fn recover(s: &Suite, source: &Path, selection: &Value, name: &str) -> Result<Value> {
    let mut selection = selection.clone();
    selection["sourceSha256"] = json!(sha(source)?);
    write(&s.root.join("selection.json"), &selection)?;
    invoke(
        &s.cli,
        &s.root,
        &[
            "recover",
            "wire-schema",
            source.to_str().ok_or("non-UTF-8 fixture path")?,
            "selection.json",
        ],
        name,
        None,
    )?;
    Ok(json!({"graph":read(&s.root.join(name).join("graph.json"))?,
        "binding":read(&s.root.join(name).join("rust.json"))?}))
}
fn rows(v: &Value) -> &Vec<Value> {
    v.as_array().expect("expected fixture array")
}
fn find<'a>(v: &'a Value, key: &str, want: &Value) -> &'a Value {
    rows(v)
        .iter()
        .find(|item| &item[key] == want)
        .expect("expected fixture declaration")
}
fn find_mut<'a>(v: &'a mut Value, key: &str, want: &Value) -> &'a mut Value {
    v.as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| &item[key] == want)
        .expect("expected fixture declaration")
}
fn hash_views(s: &Suite) -> Result {
    let model = s.json("hash-views.json")?;
    s.project(&model, "src", None)?;
    let catalog = read(&s.root.join("src/obligations.json"))?;
    for view in rows(&model["graph"]["hashViews"]) {
        assert!(
            rows(&catalog["obligations"])
                .iter()
                .any(|r| r["declaration"] == view["id"] && r["role"] == "typed-hash-view")
        );
    }
    fs::copy(s.case("hash-views-Cargo.toml"), s.root.join("Cargo.toml"))?;
    fs::copy(s.case("hash-views-oracle.rs"), s.root.join("src/main.rs"))?;
    cargo(&s.root, &["generate-lockfile", "--offline"])?;
    cargo(&s.root, &["run", "--locked", "--offline"])?;
    let mut reordered = model.clone();
    for field in ["types", "hashViews"] {
        reordered["graph"][field].as_array_mut().unwrap().reverse();
    }
    s.project(&reordered, "again", None)?;
    s.compare("src", "again", PROJECTED)?;
    s.rejections(&model, "hash-views-rejections.json")
}
fn main() -> Result {
    let (name, s) = Suite::open("wire-schema")?;
    match name.as_str() {
        "garden" => garden::verify(&s)?,
        "byte-value" => scalar::verify(&s)?,
        "hash-views" => hash_views(&s)?,
        _ => return Err("expected garden|byte-value|hash-views ODDLANG".into()),
    }
    println!("Wire schema {name} qualification passed");
    Ok(())
}
