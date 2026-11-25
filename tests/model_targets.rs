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
        (-1, Some("23514")),
        (0, None),
        (i64::MAX as i128, None),
        (i64::MAX as i128 + 1, Some("22003")),
    ] {
        let decimal = value.to_string();
        let result = db.client.execute(
            "INSERT INTO accounts (id, owner, balance) VALUES ($1, 'owner', ($1::text)::BIGINT)",
            &[&decimal],
        );
        match expected {
            Some(code) => assert_eq!(result.unwrap_err().code().unwrap().code(), code),
            None => assert_eq!(result.unwrap(), 1),
        }
    }
}
#[test]
fn equivalent_graphs_emit_identical_bytes() {
    let project = fixture();
    let before = build(project.path());
    rewrite(project.path());
    let after = build(project.path());
    for file in ["account.rs", "schema.sql", "account.avsc"] {
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
        ("rust.ttl", "None"),
        ("rust.ttl", "Some"),
        ("rust.ttl", "Ok"),
        ("rust.ttl", "Err"),
        ("rust.ttl", "Balance"),
        ("sql.ttl", oversized.as_str()),
    ] {
        let p = fixture();
        replace(p.path(), target, "\"owner\"", &format!("\"{name}\""));
        assert!(oddlang::model::load_model(p.path()).is_err(), "{name}");
    }
}

#[test]
fn postgres_checks_the_declared_upper_bound_even_when_the_value_fits_bigint() {
    let p = fixture();
    replace(p.path(), "fields.ttl", "9223372036854775807", "10");
    let out = build(p.path());
    let mut db = db::TestDatabase::connect().unwrap();
    db.client
        .batch_execute(&fs::read_to_string(out.path().join("schema.sql")).unwrap())
        .unwrap();
    let err = db
        .client
        .execute("INSERT INTO accounts VALUES ('a','b',11)", &[])
        .unwrap_err();
    assert_eq!(err.code().unwrap().code(), "23514");
}
