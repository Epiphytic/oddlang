#[path = "support/db.rs"]
mod db;
#[allow(dead_code)]
mod support_fixture;
use std::{fs, path::Path, process::Command};
use support_fixture::*;
fn build(dir: &Path) -> tempfile::TempDir {
    let out = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .arg("build")
        .arg(dir)
        .arg(out.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    out
}
#[test]
fn generated_rust_and_postgres_agree_at_both_bounds() {
    let out = build(&v1());
    let p = out.path();
    fs::write(p.join("main.rs"),r#"mod account;
use account::{Account,Balance};
fn main(){
    for (value,accepted) in [(-1,false),(0,true),(i64::MAX as i128,true),(i64::MAX as i128+1,false)]{
        assert_eq!(Balance::try_from(value).is_ok(),accepted);
        let account=Account::new("a".into(),"owner".into(),value);
        assert_eq!(account.is_ok(),accepted);
        if let Ok(a)=account {assert_eq!((a.id(),a.owner(),i128::from(a.balance())),("a","owner",value));}
    }
}"#).unwrap();
    let compile = Command::new("rustc")
        .args(["--edition=2024", "main.rs", "-o", "run"])
        .current_dir(p)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(Command::new(p.join("run")).status().unwrap().success());
    let mut db = db::TestDatabase::connect().unwrap();
    db.client
        .batch_execute(&fs::read_to_string(p.join("schema.sql")).unwrap())
        .unwrap();
    for (value, expected) in [
        (-1, false),
        (0, true),
        (i64::MAX as i128, true),
        (i64::MAX as i128 + 1, false),
    ] {
        let decimal = value.to_string();
        let result = db.client.execute(
            "INSERT INTO accounts (id, owner, balance) VALUES ($1, 'owner', ($1::text)::BIGINT)",
            &[&decimal],
        );
        assert_eq!(result.is_ok(), expected, "{decimal}: {result:?}");
    }
}
#[test]
fn equivalent_graphs_emit_identical_bytes() {
    let project = fixture();
    let before = build(project.path());
    rewrite(project.path());
    let after = build(project.path());
    for file in ["account.rs", "schema.sql"] {
        assert_eq!(
            fs::read(before.path().join(file)).unwrap(),
            fs::read(after.path().join(file)).unwrap()
        );
    }
}
#[test]
fn rejects_names_unrepresentable_by_the_target() {
    let oversized = "x".repeat(64);
    for (target, name) in [
        ("rust.ttl", "struct"),
        ("rust.ttl", "new"),
        ("sql.ttl", oversized.as_str()),
    ] {
        let p = fixture();
        replace(p.path(), target, "\"owner\"", &format!("\"{name}\""));
        assert!(oddlang::model::load_model(p.path()).is_err(), "{name}");
    }
}
