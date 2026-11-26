use super::{Dynamic, Journal, Literal, SeedValue, column, identifier, quote, seed_argument};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt::Write};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::file_profile) struct ConfiguredStorage {
    epoch_span: u64,
    retention_seconds: u64,
    floors: Vec<Floor>,
    recent: Vec<Recent>,
    transitions: Vec<Transition>,
    refresh_seed_tables: Vec<String>,
    invalid_seed: crate::file_profile::Diagnostic,
    reference: ReferenceBinding,
    native_empty_relations: Vec<String>,
    clock_maximum: u64,
    identifier_hex_digits: u8,
    timestamp_hex_digits: u8,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Floor {
    table: String,
    row_key: String,
    value: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recent {
    table: String,
    seconds: String,
    identity: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    table: String,
    field: String,
    from: String,
    to: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReferenceBinding {
    table: String,
    singleton: String,
    version_column: String,
    first_column: String,
    second_column: String,
    version: u32,
    first_domain: String,
    second_domain: String,
    mismatch: crate::file_profile::Diagnostic,
}

impl ConfiguredStorage {
    pub(in crate::file_profile) fn render(
        &self,
        journal: &Journal,
        out: &mut String,
    ) -> Result<(), String> {
        let fields = |table: &str| {
            journal
                .tables
                .iter()
                .find(|v| v.name == table)
                .map(|v| {
                    v.columns
                        .iter()
                        .map(|col| col.name.as_str())
                        .collect::<BTreeSet<_>>()
                })
                .ok_or_else(|| format!("unknown configured table {table}"))
        };
        if self.epoch_span == 0
            || self.retention_seconds == 0
            || self.floors.is_empty()
            || self.clock_maximum > i64::MAX as u64
            || !(1..=15).contains(&self.timestamp_hex_digits)
            || self.identifier_hex_digits < self.timestamp_hex_digits
            || self.clock_maximum >= (1u64 << (self.timestamp_hex_digits * 4))
            || self.native_empty_relations.is_empty()
            || !self
                .native_empty_relations
                .iter()
                .all(|name| identifier(name))
        {
            return Err("invalid configured storage horizons".into());
        }
        writeln!(
            out,
            "const RECOVERY_HORIZON: u64 = {};\nconst RECOVERY_RETENTION: u64 = {};",
            self.epoch_span, self.retention_seconds
        )
        .unwrap();
        writeln!(
            out,
            "const CLOCK_MAXIMUM: u64 = {};\nconst TIMESTAMP_WIDTH: usize = {};",
            self.clock_maximum, self.timestamp_hex_digits
        )
        .unwrap();
        writeln!(out, "const NATIVE_IDLE: &[&str] = &[").unwrap();
        for name in &self.native_empty_relations {
            let sql = format!("SELECT EXISTS(SELECT 1 FROM {})", quote(name));
            writeln!(out, "{sql:?},").unwrap();
        }
        let primary = &self.floors[0];
        let fields_for_floor = fields(&primary.table)?;
        let primary_floor = format!(
            "SELECT {} FROM {} WHERE {}=1",
            column(&primary.value, &fields_for_floor)?,
            quote(&primary.table),
            column(&primary.row_key, &fields_for_floor)?
        );
        writeln!(
            out,
            "];\nconst PRIMARY_FLOOR: &str = {primary_floor:?};\nconst ADVANCE_FLOORS: &[&str] = &["
        )
        .unwrap();
        for step in &self.floors {
            let names = fields(&step.table)?;
            let value = column(&step.value, &names)?;
            let key = column(&step.row_key, &names)?;
            let sql = format!(
                "UPDATE {} SET {value}=MAX({value},?1) WHERE {key}=1",
                quote(&step.table)
            );
            writeln!(out, "{sql:?},").unwrap();
        }
        writeln!(out, "];\nconst CHECK_RECENT: &[&str] = &[").unwrap();
        for step in &self.recent {
            let names = fields(&step.table)?;
            let seconds = column(&step.seconds, &names)?;
            let identity = column(&step.identity, &names)?;
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE {seconds}<?1 OR length({identity})!={} OR {identity} GLOB '*[^0-9a-f]*' OR substr({identity},1,{})<?2)",
                quote(&step.table),
                self.identifier_hex_digits,
                self.timestamp_hex_digits
            );
            writeln!(out, "{sql:?},").unwrap();
        }
        writeln!(out, "];\nconst TRANSITIONS: &[(&str,&str,&str)] = &[").unwrap();
        for step in &self.transitions {
            let name = column(&step.field, &fields(&step.table)?)?;
            let sql = format!(
                "UPDATE {} SET {name}=?1 WHERE {name}=?2",
                quote(&step.table)
            );
            writeln!(out, "({sql:?},{:?},{:?}),", step.from, step.to).unwrap();
        }
        writeln!(out,"];\nfn refresh_configured_seeds(tx: &rusqlite::Transaction<'_>) -> Result<(), Failure> {{").unwrap();
        let diagnostic = &self.invalid_seed;
        if diagnostic.exit == 0
            || diagnostic.stderr.is_empty()
            || diagnostic.stderr.chars().any(char::is_control)
        {
            return Err("invalid configured seed diagnostic".into());
        }
        writeln!(
            out,
            "    let invalid_seed = Failure({}, {:?});",
            diagnostic.exit, diagnostic.stderr
        )
        .unwrap();
        let mut refreshed = BTreeSet::new();
        for name in &self.refresh_seed_tables {
            let table = journal
                .tables
                .iter()
                .find(|table| table.name == *name)
                .ok_or_else(|| format!("unknown refreshed table {name}"))?;
            let seed = journal
                .seeds
                .iter()
                .find(|seed| seed.table == *name)
                .ok_or_else(|| format!("missing refreshed seed for {name}"))?;
            if !refreshed.insert(name)
                || seed.columns.len() < 2
                || seed.columns.len() != seed.values.len()
                || !table
                    .columns
                    .iter()
                    .any(|column| column.name == seed.columns[0] && column.primary)
                || seed
                    .values
                    .iter()
                    .any(|value| matches!(value, SeedValue::Dynamic(Dynamic::PublicIdentity)))
            {
                return Err(
                    "configured refresh requires a fixed seed with an inline primary key".into(),
                );
            }
            let names = fields(name)?;
            let key = column(&seed.columns[0], &names)?;
            let seed_key = seed_argument(&seed.values[0])?;
            for (field, expected) in seed.columns[1..].iter().zip(&seed.values[1..]) {
                if let SeedValue::Dynamic(Dynamic::JsonBlob { fields: expected }) = expected {
                    let field = column(field, &names)?;
                    let select = format!("SELECT {field} FROM {} WHERE {key}=?", quote(name));
                    let shape = expected
                        .iter()
                        .map(|(key, literal)| {
                            let ty = match literal {
                                Literal::Integer(_) => "unsigned",
                                Literal::Text(_) => "text",
                                Literal::Null(()) => "nullable_unsigned",
                            };
                            format!("({key:?},{ty:?})")
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    writeln!(out, "    let stored: Vec<u8> = tx.query_row({select:?}, rusqlite::params![{seed_key}], |row| row.get(0)).map_err(|_| invalid_seed)?;").unwrap();
                    writeln!(
                        out,
                        "    validate_seed_shape(&stored, &[{shape}]).ok_or(invalid_seed)?;"
                    )
                    .unwrap();
                }
            }
            let assignments = seed.columns[1..]
                .iter()
                .map(|name| Ok(format!("{}=?", column(name, &names)?)))
                .collect::<Result<Vec<_>, String>>()?
                .join(",");
            let sql = format!("UPDATE {} SET {assignments} WHERE {key}=?", quote(name));
            let parameters = seed.values[1..]
                .iter()
                .chain(seed.values[..1].iter())
                .map(seed_argument)
                .collect::<Result<Vec<_>, _>>()?
                .join(",");
            writeln!(out, "    if tx.execute({sql:?}, rusqlite::params![{parameters}]).map_err(|_| JOURNAL)? != 1 {{ return Err(JOURNAL); }}").unwrap();
        }
        writeln!(out, "    Ok(())\n}}").unwrap();
        let reference = &self.reference;
        let names = fields(&reference.table)?;
        let columns = [
            &reference.singleton,
            &reference.version_column,
            &reference.first_column,
            &reference.second_column,
        ]
        .map(|name| column(name, &names))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
        let [singleton, version, first, second] = columns.as_slice() else {
            unreachable!()
        };
        let table = quote(&reference.table);
        let read = format!("SELECT {version},{first},{second} FROM {table} WHERE {singleton}=1");
        let insert = format!(
            "INSERT INTO {table}({singleton},{version},{first},{second}) VALUES(1,?1,?2,?3)"
        );
        let rejected = &reference.mismatch;
        if rejected.exit == 0
            || rejected.stderr.is_empty()
            || rejected.stderr.chars().any(char::is_control)
            || reference.first_domain.is_empty()
            || reference.second_domain.is_empty()
            || reference.first_domain == reference.second_domain
        {
            return Err("invalid native storage reference binding".into());
        }
        writeln!(out,"const REFERENCE_QUERY: &str = {read:?};\nconst REFERENCE_INSERT: &str = {insert:?};\nconst REFERENCE_VERSION: u32 = {};\nconst REFERENCE_FIRST_DOMAIN: &str = {:?};\nconst REFERENCE_SECOND_DOMAIN: &str = {:?};\nconst REFERENCE_MISMATCH: Failure = Failure({}, {:?});",
            reference.version,reference.first_domain,reference.second_domain,rejected.exit,rejected.stderr).unwrap();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_declared_cursor_width_and_rejects_unknown_floor_column() {
        let graph = serde_json::json!({
            "version":3,
            "tables":[{"name":"register","columns":[
                {"name":"record","affinity":"integer","primary":true},
                {"name":"floor","affinity":"integer"},
                {"name":"reference","affinity":"text"},
                {"name":"first","affinity":"blob"},
                {"name":"second","affinity":"blob"}]}],
            "indexes":[],"seeds":[],"metrics":[],"empty_before_reload":[],
            "binding_table":"register","binding_column":"reference"
        });
        let journal: Journal = serde_json::from_value(graph).unwrap();
        let mut policy = serde_json::json!({
            "epoch_span":30,"retention_seconds":120,"clock_maximum":65535,
            "identifier_hex_digits":8,"timestamp_hex_digits":4,
            "native_empty_relations":["pending_tasks"],
            "floors":[{"table":"register","row_key":"record","value":"floor"}],
            "recent":[{"table":"register","seconds":"floor","identity":"reference"}],
            "transitions":[],"refresh_seed_tables":[],
            "invalid_seed":{"exit":5,"stderr":"invalid default"},
            "reference":{"table":"register","singleton":"record","version_column":"floor",
                         "first_column":"first","second_column":"second","version":4,
                         "first_domain":"urn:survey:native","second_domain":"urn:survey:external",
                         "mismatch":{"exit":3,"stderr":"bound"}}
        });
        let parse =
            |value: serde_json::Value| serde_json::from_value::<ConfiguredStorage>(value).unwrap();
        let mut output = String::new();
        parse(policy.clone()).render(&journal, &mut output).unwrap();
        assert!(output.contains("const TIMESTAMP_WIDTH: usize = 4"));
        assert!(output.contains("length(\\\"reference\\\")!=8"));
        policy["floors"][0]["value"] = "unknown".into();
        assert!(
            parse(policy)
                .render(&journal, &mut String::new())
                .unwrap_err()
                .contains("unknown native store column")
        );
    }
}
