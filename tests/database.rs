#[path = "support/db.rs"]
mod db;

use db::{TestDatabase, validate_server_version};

#[test]
fn rejects_any_other_server_release() {
    for version in ["160005", "160007", "170000"] {
        let error = validate_server_version(version).unwrap_err();
        assert!(error.contains("setup") && error.contains("160006") && error.contains(version));
    }
    assert!(validate_server_version("160006").is_ok());
}

#[test]
fn isolates_and_cleans_up_schemas() {
    let mut first = TestDatabase::connect().unwrap_or_else(|error| panic!("{error}"));
    let mut second = TestDatabase::connect().unwrap_or_else(|error| panic!("{error}"));
    let schema = first.schema.clone();

    assert_ne!(schema, second.schema);
    first
        .client
        .batch_execute("CREATE TABLE account (id bigint)")
        .unwrap();
    second
        .client
        .batch_execute("CREATE TABLE account (id bigint)")
        .unwrap();
    drop(first);

    let schema_exists: bool = second
        .client
        .query_one(
            "SELECT EXISTS (SELECT FROM pg_namespace WHERE nspname = $1::text)",
            &[&schema],
        )
        .unwrap()
        .get(0);
    assert!(!schema_exists, "schema {schema} survived cleanup");
}
