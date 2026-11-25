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
