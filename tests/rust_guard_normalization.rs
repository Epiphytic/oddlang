use oddlang::import_rust::import;
use std::{fs, path::Path};

fn import_source(source: &str, output: &Path) -> Result<Vec<u8>, String> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/ledger");
    let scratch = tempfile::tempdir().unwrap();
    let manifest = scratch.path().join("Cargo.toml");
    fs::copy(fixture.join("Cargo.toml"), &manifest).unwrap();
    fs::copy(
        fixture.join("Cargo.lock"),
        scratch.path().join("Cargo.lock"),
    )
    .unwrap();
    fs::create_dir(scratch.path().join("src")).unwrap();
    fs::write(scratch.path().join("src/lib.rs"), source).unwrap();
    import(&manifest, &fixture.join("mapping.toml"), output, None)?;
    Ok(fs::read(output.join("operations.ttl")).unwrap())
}

const SOURCE: &str = include_str!("../examples/rust-import/ledger/src/lib.rs");

#[test]
fn reversed_key_and_numeric_tests_keep_guard_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let original = import_source(SOURCE, &dir.path().join("original")).unwrap();
    let mut reversed = SOURCE.to_owned();
    for (from, to) in [
        ("source == destination", "destination == source"),
        ("debit_before < amount_wide", "amount_wide > debit_before"),
        (
            "credit_after > i128::from(i64::MAX)",
            "i128::from(i64::MAX) < credit_after",
        ),
        ("amount <= 0", "0 >= amount"),
    ] {
        assert!(reversed.contains(from));
        reversed = reversed.replace(from, to);
    }
    assert_eq!(
        original,
        import_source(&reversed, &dir.path().join("reversed")).unwrap()
    );
}

#[test]
fn unicode_local_reports_the_identifier_location() {
    let dir = tempfile::tempdir().unwrap();
    let renamed = SOURCE.replace("amount_wide", "金額");
    let line = renamed
        .lines()
        .position(|line| line.contains("let 金額"))
        .unwrap()
        + 1;
    let error = import_source(&renamed, &dir.path().join("rejected")).unwrap_err();
    assert_eq!(
        error,
        format!("src/lib.rs:{line}:9: Rust import profile v1 requires ASCII identifiers")
    );
}
