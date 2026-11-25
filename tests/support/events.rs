use apache_avro::{Schema, Writer, types::Value};
use arrow_array::{Int64Array, RecordBatch, StringArray};
use arrow_avro::{reader::ReaderBuilder, schema::AvroSchema};
use std::io::Cursor;

pub mod v1 {
    include!("../fixtures/account-v1.rs.inc");
}

pub fn write(schema: &str, values: Vec<(&str, Value)>) -> Vec<u8> {
    let schema = Schema::parse_str(schema).unwrap();
    let mut writer = Writer::new(&schema, Vec::new()).unwrap();
    writer
        .append_value(Value::Record(
            values
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        ))
        .unwrap();
    writer.into_inner().unwrap()
}

pub fn resolve(bytes: &[u8], schema: &str) -> Result<RecordBatch, String> {
    let mut reader = ReaderBuilder::new()
        .with_reader_schema(AvroSchema::new(schema.to_owned()))
        .build(Cursor::new(bytes))
        .map_err(|e| e.to_string())?;
    let batch = reader
        .next()
        .ok_or("missing event")?
        .map_err(|e| e.to_string())?;
    if batch.num_rows() != 1 || reader.next().is_some() {
        return Err("expected one event".into());
    }
    Ok(batch)
}

pub fn text(batch: &RecordBatch, name: &str) -> String {
    batch
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap()
        .value(0)
        .to_owned()
}

pub fn balance(batch: &RecordBatch) -> i128 {
    batch
        .column_by_name("balance")
        .unwrap()
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap()
        .value(0)
        .into()
}

pub fn decode_v1(bytes: &[u8], schema: &str) -> Result<v1::Account, String> {
    let row = resolve(bytes, schema)?;
    v1::Account::new(text(&row, "id"), text(&row, "owner"), balance(&row)).map_err(str::to_owned)
}

pub fn persist_v1(client: &mut postgres::Client, account: &v1::Account) -> v1::Account {
    client
        .execute(
            "INSERT INTO accounts (id, owner, balance) VALUES ($1, $2, $3)",
            &[&account.id(), &account.owner(), &account.balance()],
        )
        .unwrap();
    let row = client
        .query_one(
            "SELECT id, owner, balance FROM accounts WHERE id=$1",
            &[&account.id()],
        )
        .unwrap();
    v1::Account::new(row.get(0), row.get(1), i128::from(row.get::<_, i64>(2))).unwrap()
}

pub fn event_v1(account: &v1::Account, schema: &str) -> Vec<u8> {
    write(
        schema,
        vec![
            ("id", Value::String(account.id().into())),
            ("owner", Value::String(account.owner().into())),
            ("balance", Value::Long(account.balance())),
        ],
    )
}

pub mod v2 {
    include!("../fixtures/account-v2.rs.inc");
}

fn decode_v2(bytes: &[u8], schema: &str) -> Result<v2::Account, String> {
    let row = resolve(bytes, schema)?;
    v2::Account::new(
        text(&row, "id"),
        text(&row, "ownerName"),
        balance(&row),
        text(&row, "currencyCode"),
    )
    .map_err(str::to_owned)
}

fn read_v2(client: &mut postgres::Client, id: &str) -> v2::Account {
    let row = client
        .query_one(
            "SELECT id, owner_name, balance, currency_code FROM accounts WHERE id=$1",
            &[&id],
        )
        .unwrap();
    v2::Account::new(
        row.get(0),
        row.get(1),
        i128::from(row.get::<_, i64>(2)),
        row.get(3),
    )
    .unwrap()
}

pub struct Witness {
    pub historical: v2::Account,
    pub current: v2::Account,
    pub missing_default: String,
}

pub fn evolution_witness(
    client: &mut postgres::Client,
    writer: &oddlang::model::Model,
    reader: &oddlang::model::Model,
) -> Witness {
    use oddlang::{evolution, model::OWNER, targets};
    const CURRENCY: &str = "urn:oddlang:ledger:currencyCode";
    assert!(
        targets::rust::generate(writer).starts_with(include_str!("../fixtures/account-v1.rs.inc"))
    );
    assert!(
        targets::rust::generate(reader).starts_with(include_str!("../fixtures/account-v2.rs.inc"))
    );
    evolution::wire_compatibility(writer, reader).unwrap();
    client
        .batch_execute(&targets::postgres::generate(writer))
        .unwrap();
    let original = v1::Account::new("historical".into(), "Zoë".into(), i64::MAX.into()).unwrap();
    let stored = persist_v1(client, &original);
    assert_eq!(stored, original);
    let writer_schema = targets::avro::generate(writer);
    let bytes = event_v1(&stored, &writer_schema);
    assert_eq!(decode_v1(&bytes, &writer_schema).unwrap(), original);
    client
        .batch_execute(&evolution::migration(writer, reader).unwrap())
        .unwrap();
    let schema = targets::avro::generate(reader);
    let historical = decode_v2(&bytes, &schema).unwrap();
    assert_eq!(historical, read_v2(client, original.id()));
    assert_eq!(
        historical,
        v2::Account::new(
            original.id().into(),
            original.owner().into(),
            original.balance().into(),
            "CAD".into()
        )
        .unwrap()
    );

    let mut no_alias = reader.clone();
    no_alias.fields.get_mut(OWNER).unwrap().aliases.clear();
    assert_eq!(
        evolution::wire_compatibility(writer, &no_alias).unwrap_err(),
        format!("{OWNER}: missing reader alias owner")
    );
    assert!(resolve(&bytes, &targets::avro::generate(&no_alias)).is_err());

    let mut no_default = reader.clone();
    no_default
        .fields
        .get_mut(CURRENCY)
        .unwrap()
        .historical_value = None;
    let missing_default = evolution::wire_compatibility(writer, &no_default).unwrap_err();
    assert_eq!(
        missing_default,
        format!("{CURRENCY}: missing reader default")
    );
    assert!(resolve(&bytes, &targets::avro::generate(&no_default)).is_err());

    let expected = v2::Account::new("new".into(), "Mio".into(), 57, "JPY".into()).unwrap();
    client
        .execute(
            "INSERT INTO accounts (id,owner_name,balance,currency_code) VALUES ($1,$2,$3,$4)",
            &[
                &expected.id(),
                &expected.owner(),
                &expected.balance(),
                &expected.currency_code(),
            ],
        )
        .unwrap();
    let stored = read_v2(client, expected.id());
    assert_eq!(stored, expected);
    let bytes = write(
        &schema,
        vec![
            ("id", Value::String(stored.id().into())),
            ("ownerName", Value::String(stored.owner().into())),
            ("balance", Value::Long(stored.balance())),
            ("currencyCode", Value::String(stored.currency_code().into())),
        ],
    );
    let current = decode_v2(&bytes, &schema).unwrap();
    assert_eq!(current, expected);
    Witness {
        historical,
        current,
        missing_default,
    }
}
