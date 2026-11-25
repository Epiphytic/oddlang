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
