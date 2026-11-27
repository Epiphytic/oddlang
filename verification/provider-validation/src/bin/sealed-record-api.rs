//! Attach a separately pinned source API to an immutable recovered record.
use provider_validation::{Result, cargo, cli, invoke, pin, read, sha, write};
use serde_json::{Value, json};
use std::{fs, path::Path};
const FIELD: &str = "urn:parking:stay:departure";
fn bundle(root: &Path, folder: &str, format: &str, g: &Value, b: &Value) -> Result<Value> {
    let dir = root.join(folder);
    fs::create_dir(&dir)?;
    write(&dir.join("graph.json"), g)?;
    write(&dir.join("rust.json"), b)?;
    write(
        &dir.join("recovery.lock"),
        &json!({"format":format,"graphSha256":sha(&dir.join("graph.json"))?,"overlaySha256":sha(&dir.join("rust.json"))?,
        "sourceSha256":"a".repeat(64),"selectionSha256":"b".repeat(64),"crateRootSha256":"c".repeat(64),"rustdocSha256":"d".repeat(64)}),
    )?;
    Ok(json!({"recovery":pin(root,&format!("{folder}/recovery.lock"))?}))
}
fn with_role(record: &Value, role: &Value) -> Value {
    let mut r = record.clone();
    r["sourceApi"] = role.clone();
    r
}
fn main() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let zone = "urn:parking:zone";
    let north = "urn:parking:north";
    let stay = "urn:parking:stay";
    let module = "urn:parking:records";
    let mut choice = bundle(
        root,
        "zone",
        "oddlang-rust-serde-string-choice-recovery-v1",
        &json!({"format":"oddlang-shared-declarations-v1","id":"urn:parking:zones","stringEnums":[{"id":zone,"variants":[{"id":north,"wire":"North"}]}]}),
        &json!({"format":"oddlang-shared-rust-v1","names":{zone:"Zone",north:"North"},"variantOrder":{zone:[north]}}),
    )?;
    choice["module"] = json!("parking");
    let graph = json!({"format":"oddlang-public-records-v1","modules":[{"id":module,"aliases":[],"records":[{
        "id":stay,"traits":["serialize","deserialize"],"readPolicy":{"shape":"object-or-ordered-sequence","unknownFields":"ignore"},
        "fields":[{"id":FIELD,"wire":"departure","type":{"optional":"u64"}},{"id":"urn:parking:stay:zone","wire":"zone","type":{"external":zone}}]
    }]}]});
    let binding = json!({"format":"oddlang-public-records-rust-v1","flattenModule":true,"identifiers":{module:"parking",stay:"Stay"},"externalTypes":{zone:["crate","Zone"]}});
    let format = "oddlang-serde-public-record-recovery-lock-v1";
    let record = bundle(root, "stay", format, &graph, &binding)?;
    let role = json!({"role":"rust-drop-in-source-api","fields":{FIELD:{"name":"departure","visibility":"public","type":{"optional":"u64"}}}});
    let full = with_role(&binding, &role);
    write(&root.join("role.json"), &full)?;
    let pinned = pin(root, "role.json")?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='parking_api'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nserde={version='=1.0.229',features=['derive']}\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    fs::remove_file(root.join("src/lib.rs"))?;
    let lock = pin(root, "Cargo.lock")?;
    let build = |label: &str, components: Value, error: Option<&str>| -> Result {
        write(
            &root.join("package.json"),
            &json!({"format":"oddlang-native-package-v1","name":"parking_api","version":"0.1.0","edition":"2024",
            "cargoLock":lock,"dependencies":{"serde":{"version":"1.0.229","features":["derive"]}},"components":components}),
        )?;
        invoke(
            &cli,
            root,
            &["build", "native-package", "rust", "package.json"],
            label,
            error,
        )
    };
    build("baseline", json!([choice, record]), None)?;
    build(
        "with-role",
        json!([choice, with_role(&record, &pinned)]),
        None,
    )?;
    let product = root.join("with-role");
    let path = "src/group_0/__projection_1.rs";
    assert_eq!(
        fs::read(product.join(path))?,
        fs::read(root.join("baseline").join(path))?
    );
    let product_lock = read(&product.join("package.lock"))?;
    let entry = &product_lock["components"][1];
    assert_eq!(entry["overlaySha256"], sha(&root.join("stay/rust.json"))?);
    assert_eq!(
        entry["recoverySha256"],
        sha(&root.join("stay/recovery.lock"))?
    );
    assert_eq!(entry["sourceApiSha256"], pinned["sha256"]);
    assert_eq!(entry["projection"]["sourceApiSha256"], pinned["sha256"]);
    fs::create_dir(product.join("tests"))?;
    fs::write(
        product.join("tests/caller.rs"),
        include_str!("../../../recovered-public-data/parking-caller.rs"),
    )?;
    let observed = String::from_utf8(cargo(&product, &["test", "--offline", "--locked"])?)?;
    assert!(observed.contains("test source_shape ... ok"));
    for (label, pointer, value, error) in [
        ("wrong-role", "/sourceApi/role", "wire", "unknown variant"),
        (
            "wrong-name",
            "/sourceApi/fields/urn:parking:stay:departure/name",
            "wrong",
            "source API field name differs",
        ),
        (
            "drift",
            "/identifiers/urn:parking:stay",
            "Different",
            "differs from recovered binding",
        ),
    ] {
        let mut bad = full.clone();
        *bad.pointer_mut(pointer).unwrap() = json!(value);
        write(&root.join("bad.json"), &bad)?;
        build(
            label,
            json!([choice, with_role(&record, &pin(root, "bad.json")?)]),
            Some(error),
        )?;
    }
    write(&root.join("bad.json"), &binding)?;
    build(
        "missing-role",
        json!([choice, with_role(&record, &pin(root, "bad.json")?)]),
        Some("requires sourceApi"),
    )?;
    let mut stale = pinned.clone();
    stale["sha256"] = json!("0".repeat(64));
    build(
        "stale",
        json!([choice, with_role(&record, &stale)]),
        Some("input digest mismatch"),
    )?;
    build(
        "choice-role",
        json!([with_role(&choice, &pinned), record]),
        Some("only valid for recovered public records"),
    )?;
    let embedded = bundle(root, "embedded", format, &graph, &full)?;
    build(
        "embedded-and-separate",
        json!([choice, with_role(&embedded, &pinned)]),
        Some("already has a source API"),
    )?;
    println!(
        "Pinned separate API binding and immutable recovery compose with unchanged native output"
    );
    Ok(())
}
