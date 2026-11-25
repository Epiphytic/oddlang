use oddlang::{model::load_model, targets};
#[allow(dead_code)]
mod support_fixture;

#[test]
fn builds_pinned_v2_event_schema() {
    let model = load_model("examples/ledger/v2").unwrap();
    let dir = tempfile::tempdir().unwrap();
    targets::build(&model, dir.path()).unwrap();
    let schema = std::fs::read_to_string(dir.path().join("account.avsc")).unwrap();
    assert_eq!(schema, include_str!("fixtures/evolution/account-v2.avsc"));
    let project = v2_fixture();
    support_fixture::rewrite(project.path());
    let reordered = load_model(project.path()).unwrap();
    assert_eq!(targets::avro::generate(&reordered), schema);
}

#[path = "support/db.rs"]
mod db;
#[path = "support/events.rs"]
mod events;

#[test]
fn checked_account_survives_sql_and_event() {
    use apache_avro::types::Value;
    let model = load_model("examples/ledger/v1").unwrap();
    let source = targets::rust::generate(&model);
    assert!(source.starts_with(include_str!("fixtures/account-v1.rs.inc")));
    let mut database = db::TestDatabase::connect().unwrap();
    database
        .client
        .batch_execute(&targets::postgres::generate(&model))
        .unwrap();
    let expected = events::v1::Account::new("A".into(), "Renée".into(), i64::MAX.into()).unwrap();
    let selected = events::persist_v1(&mut database.client, &expected);
    assert_eq!(selected, expected);
    let schema = targets::avro::generate(&model);
    let bytes = events::event_v1(&selected, &schema);
    assert_eq!(events::decode_v1(&bytes, &schema).unwrap(), expected);
    let invalid = events::write(
        &schema,
        vec![
            ("id", Value::String("B".into())),
            ("owner", Value::String("Ada".into())),
            ("balance", Value::Long(-1)),
        ],
    );
    assert_eq!(
        events::decode_v1(&invalid, &schema).unwrap_err(),
        oddlang::model::BALANCE
    );
}

#[test]
fn migrates_populated_rows() {
    let v1 = load_model("examples/ledger/v1").unwrap();
    let v2 = load_model("examples/ledger/v2").unwrap();
    let mut db = db::TestDatabase::connect().unwrap();
    db.client
        .batch_execute(&targets::postgres::generate(&v1))
        .unwrap();
    db.client
        .batch_execute("INSERT INTO accounts VALUES ('a','Ada',0),('b','Ben',9223372036854775807)")
        .unwrap();
    let sql = oddlang::evolution::migration(&v1, &v2).unwrap();
    db.client.batch_execute(&sql).unwrap();
    let rows: Vec<(String, String, i64, String)> = db
        .client
        .query(
            "SELECT id,owner_name,balance,currency_code FROM accounts ORDER BY id",
            &[],
        )
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("a".into(), "Ada".into(), 0, "CAD".into()),
            ("b".into(), "Ben".into(), i64::MAX, "CAD".into())
        ]
    );
    let mut fresh = db::TestDatabase::connect().unwrap();
    fresh
        .client
        .batch_execute(&targets::postgres::generate(&v2))
        .unwrap();
    for catalog in [
        "SELECT concat_ws('|',column_name,data_type,is_nullable,COALESCE(column_default,'<none>')) FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='accounts' ORDER BY ordinal_position",
        "SELECT concat_ws('|',contype,pg_get_constraintdef(oid)) FROM pg_constraint WHERE conrelid='accounts'::regclass ORDER BY 1",
    ] {
        let entries = |client: &mut postgres::Client| {
            client
                .query(catalog, &[])
                .unwrap()
                .iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>()
        };
        assert_eq!(entries(&mut db.client), entries(&mut fresh.client));
    }
    let error = db
        .client
        .execute(
            "INSERT INTO accounts(id,owner_name,balance) VALUES ('c','Cam',22)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        error.code(),
        Some(&postgres::error::SqlState::NOT_NULL_VIOLATION)
    );
    assert_eq!(
        db.client
            .query_one(
                "INSERT INTO accounts VALUES ('c','Cam',22,'JPY') RETURNING currency_code",
                &[]
            )
            .unwrap()
            .get::<_, String>(0),
        "JPY"
    );
}

#[test]
fn requires_historical_value_before_migration() {
    let writer = load_model("examples/ledger/v1").unwrap();
    let mut reader = load_model("examples/ledger/v2").unwrap();
    let iri = "urn:oddlang:ledger:currencyCode";
    reader.fields.get_mut(iri).unwrap().historical_value = None;
    assert_eq!(
        oddlang::evolution::migration(&writer, &reader).unwrap_err(),
        format!("{iri}: missing historical value")
    );
}

#[test]
fn independent_resolver_confirms_compatibility_verdicts() {
    let writer = load_model("examples/ledger/v1").unwrap();
    let reader = load_model("examples/ledger/v2").unwrap();
    let mut db = db::TestDatabase::connect().unwrap();
    let witness = events::evolution_witness(&mut db.client, &writer, &reader);
    assert_eq!(witness.current.currency_code(), "JPY");
    assert_eq!(witness.historical.currency_code(), "CAD");
    assert!(witness.missing_default.ends_with("missing reader default"));
}

fn v2_fixture() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    for file in std::fs::read_dir("examples/ledger/v2").unwrap() {
        let file = file.unwrap();
        std::fs::copy(file.path(), project.path().join(file.file_name())).unwrap();
    }
    project
}

#[test]
fn reads_directional_negative_fixtures_from_disk() {
    let writer = load_model("examples/ledger/v1").unwrap();
    let account = events::v1::Account::new("id".into(), "owner".into(), 1).unwrap();
    let bytes = events::event_v1(&account, &targets::avro::generate(&writer));
    for (variant, file, expected, wire_name) in [
        (
            "no-alias",
            "avro.ttl",
            "urn:oddlang:ledger:owner: missing reader alias owner",
            "ownerName",
        ),
        (
            "no-default",
            "domain.ttl",
            "urn:oddlang:ledger:currencyCode: missing reader default",
            "currencyCode",
        ),
    ] {
        let project = v2_fixture();
        std::fs::copy(
            format!("tests/fixtures/evolution/{variant}/{file}"),
            project.path().join(file),
        )
        .unwrap();
        let reader = load_model(project.path()).unwrap();
        assert_eq!(
            oddlang::evolution::wire_compatibility(&writer, &reader).unwrap_err(),
            expected
        );
        let resolver_error =
            events::resolve(&bytes, &targets::avro::generate(&reader)).unwrap_err();
        assert!(resolver_error.contains(wire_name), "{resolver_error}");
    }
}

#[test]
fn rejects_invalid_and_ambiguous_aliases() {
    for alias in ["not a name", "ownerName", "id"] {
        let project = v2_fixture();
        let path = project.path().join("avro.ttl");
        let data = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            data.replace("o:alias \"owner\"", &format!("o:alias \"{alias}\"")),
        )
        .unwrap();
        let error = load_model(project.path()).unwrap_err();
        assert!(error.contains(alias), "{error}");
    }
}

#[test]
fn rejects_reverse_transition_context() {
    let project = v2_fixture();
    let path = project.path().join("v1-to-v2.ttl");
    let data = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, data.replace("l:v1 ;", "l:v2 ;")).unwrap();
    assert!(
        load_model(project.path())
            .unwrap_err()
            .contains("sourceVersion")
    );
}

#[test]
fn rollback_restores_original_columns_after_mid_migration_failure() {
    let writer = load_model("examples/ledger/v1").unwrap();
    let reader = load_model("examples/ledger/v2").unwrap();
    let mut db = db::TestDatabase::connect().unwrap();
    db.client
        .batch_execute(&targets::postgres::generate(&writer))
        .unwrap();
    db.client
        .batch_execute("ALTER TABLE accounts ADD COLUMN currency_code text")
        .unwrap();
    let sql = oddlang::evolution::migration(&writer, &reader).unwrap();
    assert_eq!(
        db.client.batch_execute(&sql).unwrap_err().code(),
        Some(&postgres::error::SqlState::DUPLICATE_COLUMN)
    );
    db.client.batch_execute("ROLLBACK").unwrap();
    db.client.query("SELECT owner FROM accounts", &[]).unwrap();
}

#[test]
fn rejects_backfill_on_existing_writer_field() {
    let writer = load_model("examples/ledger/v1").unwrap();
    let mut reader = load_model("examples/ledger/v2").unwrap();
    reader
        .fields
        .get_mut(oddlang::model::OWNER)
        .unwrap()
        .historical_value = Some("Alice".into());
    assert_eq!(
        oddlang::evolution::migration(&writer, &reader).unwrap_err(),
        "urn:oddlang:ledger:owner: historical backfill targets existing writer field"
    );
}
