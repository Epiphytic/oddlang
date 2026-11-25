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

/// Check stable identities against Avro's reader-side name, alias and default rules.
/// Removed writer fields can be skipped; added reader fields need a default.
pub fn wire_compatibility(writer: &Model, reader: &Model) -> Result<(), String> {
    if writer.avro_name != reader.avro_name {
        return Err(format!("{ACCOUNT}: record name changed"));
    }
    for (iri, field) in &reader.fields {
        let matches =
            |name: &str| field.avro_name == name || field.aliases.iter().any(|a| a == name);
        if writer
            .fields
            .iter()
            .any(|(id, old)| id != iri && matches(&old.avro_name))
        {
            return Err(format!(
                "{iri}: wire name resolves to a different field identity"
            ));
        }
        if let Some(old) = writer.fields.get(iri) {
            if !matches(&old.avro_name) {
                return Err(format!("{iri}: missing reader alias {}", old.avro_name));
            }
            if old.range.is_some() != field.range.is_some() {
                return Err(format!("{iri}: incompatible wire type"));
            }
        } else if field.historical_value.is_none() {
            return Err(format!("{iri}: missing reader default"));
        }
    }
    Ok(())
}
