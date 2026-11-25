//! Directional evolution of ledger storage and event fields by stable IRI.
use crate::model::{ACCOUNT, Model};
use std::fmt::Write;

/// Generate the supported rename and required-text addition in one transaction.
pub fn migration(writer: &Model, reader: &Model) -> Result<String, String> {
    if writer.sql_name != reader.sql_name || writer.key != reader.key {
        return Err(format!("{ACCOUNT}: unsupported table or key change"));
    }
    let mut sql = String::from("BEGIN;\n");
    let table = format!("\"{}\"", reader.sql_name);
    for (iri, old) in &writer.fields {
        let new = reader
            .fields
            .get(iri)
            .ok_or_else(|| format!("{iri}: unsupported field removal"))?;
        if old.range != new.range {
            return Err(format!("{iri}: unsupported range change"));
        }
        if old.sql_name != new.sql_name {
            writeln!(
                sql,
                "ALTER TABLE {table} RENAME COLUMN \"{}\" TO \"{}\";",
                old.sql_name, new.sql_name
            )
            .unwrap();
        }
    }
    for (iri, new) in &reader.fields {
        if writer.fields.contains_key(iri) {
            continue;
        }
        let value = new
            .historical_value
            .as_ref()
            .ok_or_else(|| format!("{iri}: missing historical value"))?;
        let column = format!("\"{}\"", new.sql_name);
        let value = value.replace('\\', "\\\\").replace('\'', "''");
        writeln!(sql, "ALTER TABLE {table} ADD COLUMN {column} TEXT;").unwrap();
        writeln!(sql, "UPDATE {table} SET {column} = E'{value}';").unwrap();
        writeln!(
            sql,
            "ALTER TABLE {table} ALTER COLUMN {column} SET NOT NULL;"
        )
        .unwrap();
    }
    sql.push_str("COMMIT;\n");
    Ok(sql)
}
