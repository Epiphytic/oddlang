//! Restricted SQLite schema declarations for native file profile providers.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub(super) version: u32,
    tables: Vec<Table>,
    indexes: Vec<Index>,
    seeds: Vec<Seed>,
    metrics: Vec<Metric>,
    empty_before_reload: Vec<String>,
    binding_table: String,
    binding_column: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Table {
    name: String,
    columns: Vec<Column>,
    #[serde(default)]
    constraints: Vec<Constraint>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Column {
    name: String,
    affinity: Affinity,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    autoincrement: bool,
    #[serde(default)]
    unique: bool,
    default: Option<Literal>,
    check: Option<Predicate>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Affinity {
    Integer,
    Text,
    Blob,
}

impl Affinity {
    fn sql(&self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Constraint {
    Primary { columns: Vec<String> },
    Unique { columns: Vec<String> },
    Check { predicate: Predicate },
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Predicate {
    Equal {
        value: Operand,
        expected: Literal,
    },
    AtLeast {
        value: Operand,
        minimum: i64,
    },
    Between {
        value: Operand,
        minimum: i64,
        maximum: i64,
    },
    In {
        value: Operand,
        choices: Vec<Literal>,
    },
    Present {
        column: String,
    },
    All {
        predicates: Vec<Predicate>,
    },
    Any {
        predicates: Vec<Predicate>,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operand {
    Column { column: String },
    ByteLength { column: String },
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Literal {
    Integer(i64),
    Text(String),
    Null(()),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Index {
    name: String,
    table: String,
    columns: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    table: String,
    columns: Vec<String>,
    values: Vec<SeedValue>,
}
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum SeedValue {
    Literal(Literal),
    Dynamic(Dynamic),
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Dynamic {
    PublicIdentity,
    JsonBlob { fields: Vec<(String, Literal)> },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Metric {
    key: String,
    table: String,
    predicate: Option<Predicate>,
}

fn identifier(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && !name.to_ascii_lowercase().starts_with("sqlite_")
}

fn quote(name: &str) -> String {
    format!("\"{name}\"")
}

fn sql_literal(value: &Literal) -> Result<String, String> {
    Ok(match value {
        Literal::Integer(v) => v.to_string(),
        Literal::Null(()) => "NULL".into(),
        Literal::Text(v) if !v.contains('\0') => format!("'{}'", v.replace('\'', "''")),
        Literal::Text(_) => return Err("NUL in native store literal".into()),
    })
}

fn column(name: &str, fields: &BTreeSet<&str>) -> Result<String, String> {
    if !fields.contains(name) {
        return Err(format!("unknown native store column {name}"));
    }
    Ok(quote(name))
}

fn columns(names: &[String], fields: &BTreeSet<&str>) -> Result<String, String> {
    if names.is_empty() || names.iter().collect::<BTreeSet<_>>().len() != names.len() {
        return Err("empty or duplicate native store column list".into());
    }
    Ok(names
        .iter()
        .map(|name| column(name, fields))
        .collect::<Result<Vec<_>, _>>()?
        .join(","))
}

impl Operand {
    fn sql(&self, fields: &BTreeSet<&str>) -> Result<String, String> {
        Ok(match self {
            Self::Column { column: name } => column(name, fields)?,
            Self::ByteLength { column: name } => format!("length({})", column(name, fields)?),
        })
    }
}
impl Predicate {
    fn sql(&self, fields: &BTreeSet<&str>) -> Result<String, String> {
        Ok(match self {
            Self::Equal { value, expected } => {
                format!("{}={}", value.sql(fields)?, sql_literal(expected)?)
            }
            Self::AtLeast { value, minimum } => format!("{}>={minimum}", value.sql(fields)?),
            Self::Between {
                value,
                minimum,
                maximum,
            } if minimum <= maximum => {
                format!("{} BETWEEN {minimum} AND {maximum}", value.sql(fields)?)
            }
            Self::In { value, choices } if !choices.is_empty() => format!(
                "{} IN ({})",
                value.sql(fields)?,
                choices
                    .iter()
                    .map(sql_literal)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            ),
            Self::Present { column: name } => format!("{} IS NOT NULL", column(name, fields)?),
            Self::All { predicates } | Self::Any { predicates } if predicates.len() >= 2 => {
                let join = if matches!(self, Self::All { .. }) {
                    " AND "
                } else {
                    " OR "
                };
                format!(
                    "({})",
                    predicates
                        .iter()
                        .map(|part| part.sql(fields))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(join)
                )
            }
            _ => return Err("incomplete native store predicate".into()),
        })
    }
}

impl Journal {
    pub(super) fn render(&self, identity_key: &str) -> Result<String, String> {
        if self.version == 0
            || self.version > i32::MAX as u32
            || self.tables.is_empty()
            || self.metrics.is_empty()
        {
            return Err("invalid native journal dimensions".into());
        }
        let mut schemas = BTreeMap::new();
        let mut objects = BTreeSet::new();
        let mut ddl = String::new();
        for table in &self.tables {
            if !identifier(&table.name)
                || !objects.insert(table.name.to_ascii_lowercase())
                || table.columns.is_empty()
            {
                return Err("invalid native journal table".into());
            }
            let names: BTreeSet<_> = table
                .columns
                .iter()
                .map(|col| col.name.to_ascii_lowercase())
                .collect();
            if names.len() != table.columns.len()
                || !table.columns.iter().all(|col| identifier(&col.name))
            {
                return Err("invalid native journal column".into());
            }
            let fields: BTreeSet<_> = table.columns.iter().map(|col| col.name.as_str()).collect();
            let mut definitions = Vec::new();
            let primary = table.columns.iter().filter(|col| col.primary).count()
                + table
                    .constraints
                    .iter()
                    .filter(|constraint| matches!(constraint, Constraint::Primary { .. }))
                    .count();
            if primary > 1 {
                return Err("duplicate native journal primary key".into());
            }
            for field in &table.columns {
                let mut definition = format!("{} {}", quote(&field.name), field.affinity.sql());
                if field.primary {
                    definition.push_str(" PRIMARY KEY");
                }
                if field.autoincrement {
                    if !field.primary || !matches!(field.affinity, Affinity::Integer) {
                        return Err("autoincrement requires an integer primary key".into());
                    }
                    definition.push_str(" AUTOINCREMENT");
                }
                if field.required {
                    definition.push_str(" NOT NULL");
                }
                if field.unique {
                    definition.push_str(" UNIQUE");
                }
                if let Some(default) = &field.default {
                    write!(definition, " DEFAULT {}", sql_literal(default)?).unwrap();
                }
                if let Some(check) = &field.check {
                    write!(definition, " CHECK({})", check.sql(&fields)?).unwrap();
                }
                definitions.push(definition);
            }
            for rule in &table.constraints {
                let rule = match rule {
                    Constraint::Primary { columns: names } => {
                        format!("PRIMARY KEY({})", columns(names, &fields)?)
                    }
                    Constraint::Unique { columns: names } => {
                        format!("UNIQUE({})", columns(names, &fields)?)
                    }
                    Constraint::Check { predicate } => {
                        format!("CHECK({})", predicate.sql(&fields)?)
                    }
                };
                definitions.push(rule);
            }
            writeln!(
                ddl,
                "CREATE TABLE IF NOT EXISTS {}({});",
                quote(&table.name),
                definitions.join(",")
            )
            .unwrap();
            schemas.insert(table.name.as_str(), fields);
        }
        let table = |name: &str| {
            schemas
                .get(name)
                .ok_or_else(|| format!("unknown native journal table {name}"))
        };
        for index in &self.indexes {
            if !identifier(&index.name) || !objects.insert(index.name.to_ascii_lowercase()) {
                return Err("invalid native journal index".into());
            }
            writeln!(
                ddl,
                "CREATE INDEX IF NOT EXISTS {} ON {}({});",
                quote(&index.name),
                quote(&index.table),
                columns(&index.columns, table(&index.table)?)?
            )
            .unwrap();
        }
        let mut text = format!(
            "const JOURNAL_DDL: &str = {ddl:?};\nconst JOURNAL_VERSION: u32 = {};\n",
            self.version
        );
        let binding = column(&self.binding_column, table(&self.binding_table)?)?;
        if !self.seeds.iter().any(|seed| {
            seed.table == self.binding_table
                && seed.columns.len() == 1
                && seed.columns[0] == self.binding_column
                && matches!(
                    seed.values.as_slice(),
                    [SeedValue::Dynamic(Dynamic::PublicIdentity)]
                )
        }) {
            return Err("missing native journal identity seed".into());
        }
        let check_binding = format!(
            "SELECT {binding} FROM {} LIMIT 2",
            quote(&self.binding_table)
        );
        writeln!(text, "const JOURNAL_IDENTITY: &str = {check_binding:?};").unwrap();
        writeln!(text, "fn native_journal_seed(tx: &rusqlite::Transaction<'_>, account: &str) -> Result<(), Failure> {{").unwrap();
        let mut seeded = BTreeSet::new();
        for seed in &self.seeds {
            if seed.columns.len() != seed.values.len() || !seeded.insert(&seed.table) {
                return Err("invalid or duplicate native journal seed".into());
            }
            let names = columns(&seed.columns, table(&seed.table)?)?;
            let variables = vec!["?"; seed.values.len()].join(",");
            let query = if seed.table == self.binding_table {
                format!(
                    "INSERT INTO {}({names}) SELECT {variables} WHERE NOT EXISTS (SELECT 1 FROM {})",
                    quote(&seed.table),
                    quote(&seed.table)
                )
            } else {
                format!(
                    "INSERT OR IGNORE INTO {}({names}) VALUES({variables})",
                    quote(&seed.table)
                )
            };
            let arguments = seed
                .values
                .iter()
                .map(seed_argument)
                .collect::<Result<Vec<_>, _>>()?
                .join(",");
            writeln!(
                text,
                "    tx.execute({query:?}, rusqlite::params![{arguments}]).map_err(|_| JOURNAL)?;"
            )
            .unwrap();
        }
        text.push_str("    Ok(())\n}\n");
        let mut names = BTreeSet::from([identity_key]);
        writeln!(text, "const METRICS: &[(&str,&str)] = &[").unwrap();
        for metric in &self.metrics {
            if !identifier(&metric.key) || !names.insert(metric.key.as_str()) {
                return Err("invalid native journal metric".into());
            }
            let fields = table(&metric.table)?;
            let selector = metric
                .predicate
                .as_ref()
                .map(|expr| expr.sql(fields))
                .transpose()?
                .map_or_else(String::new, |expr| format!(" WHERE {expr}"));
            let query = format!("SELECT count(*) FROM {}{selector}", quote(&metric.table));
            writeln!(text, "    ({:?}, {query:?}),", metric.key).unwrap();
        }
        writeln!(text, "];\nconst RELOAD_GUARDS: &[&str] = &[").unwrap();
        for name in &self.empty_before_reload {
            table(name)?;
            let query = format!("SELECT EXISTS (SELECT 1 FROM {})", quote(name));
            writeln!(text, "    {query:?},").unwrap();
        }
        text.push_str("];\n");
        Ok(text)
    }
}

fn seed_argument(value: &SeedValue) -> Result<String, String> {
    Ok(match value {
        SeedValue::Literal(Literal::Integer(v)) => format!("{v}i64"),
        SeedValue::Literal(Literal::Text(v)) => format!("{v:?}"),
        SeedValue::Literal(Literal::Null(())) => "rusqlite::types::Null".into(),
        SeedValue::Dynamic(Dynamic::PublicIdentity) => "account".into(),
        SeedValue::Dynamic(Dynamic::JsonBlob { fields }) => {
            let mut names = BTreeSet::new();
            let pairs = fields
                .iter()
                .map(|(key, value)| {
                    if key.is_empty() || !names.insert(key) {
                        return Err("invalid native journal JSON seed field".to_owned());
                    }
                    Ok(format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        serde_json::to_string(value).unwrap()
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;
            format!("&{:?}.as_bytes()", format!("{{{}}}", pairs.join(",")))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_undeclared_identifiers_and_projects_parameterized_rows() {
        let mut graph = serde_json::json!({
            "version": 5,
            "tables": [{"name":"racks","columns":[
                {"name":"owner","affinity":"text","required":true},
                {"name":"weight","affinity":"integer","default":31,
                    "check":{"kind":"at_least","value":{"kind":"column","column":"weight"},"minimum":7}}
            ]}],
            "indexes": [{"name":"rack_weights","table":"racks","columns":["weight"]}],
            "seeds":[{"table":"racks","columns":["owner"],"values":[{"kind":"public_identity"}]}],
            "metrics":[{"key":"occupied","table":"racks",
                "predicate":{"kind":"present","column":"weight"}}],
            "empty_before_reload":[],
            "binding_table":"racks",
            "binding_column":"owner"
        });
        let rendered = serde_json::from_value::<Journal>(graph.clone())
            .unwrap()
            .render("holder")
            .unwrap();
        assert!(rendered.contains("JOURNAL_VERSION: u32 = 5"));
        assert!(
            rendered
                .contains("SELECT count(*) FROM \\\"racks\\\" WHERE \\\"weight\\\" IS NOT NULL")
        );
        assert!(rendered.contains("SELECT ? WHERE NOT EXISTS"));
        graph["metrics"][0]["predicate"]["column"] = "absent".into();
        assert!(
            serde_json::from_value::<Journal>(graph.clone())
                .unwrap()
                .render("holder")
                .unwrap_err()
                .contains("unknown native store column")
        );
        graph["metrics"][0]["predicate"]["column"] = "weight".into();
        graph["tables"][0]["name"] = "racks; DROP TABLE racks".into();
        assert!(
            serde_json::from_value::<Journal>(graph)
                .unwrap()
                .render("holder")
                .is_err()
        );
    }
}
