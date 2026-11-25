use oddlang::{model::load_model, targets};

#[test]
fn builds_event_schema() {
    let model = load_model("examples/ledger/v1").unwrap();
    let dir = tempfile::tempdir().unwrap();
    targets::build(&model, dir.path()).unwrap();
    let schema = std::fs::read_to_string(dir.path().join("account.avsc")).unwrap();
    assert!(schema.contains("balance"));
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
