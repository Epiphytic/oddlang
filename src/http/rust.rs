//! Project the finite ledger HTTP profile into an Axum service binding.
use super::{Constraint, HttpContract, HttpError, Iri};
use std::{fmt::Write, fs, path::Path};

fn identifier(name: &str) -> Result<&str, String> {
    let mut bytes = name.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(format!("unsupported Rust identifier {name:?}"));
    }
    Ok(name)
}
fn record_name(id: &Iri) -> Result<&str, String> {
    identifier(id.as_str().rsplit(':').next().unwrap())
}

pub fn emit_rust(c: &HttpContract, out: &Path) -> Result<(), HttpError> {
    let mut source = String::from("// Generated from the pinned HTTP contract.\n");
    for (id, record) in &c.records {
        writeln!(source,"#[derive(serde::Serialize,serde::Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct {} {{",record_name(id)?).unwrap();
        for field in &record.fields {
            let f = &c.fields[field];
            let ty = match f.shape.as_str() {
                "urn:oddlang:http:Text" => "String".into(),
                "urn:oddlang:http:Boolean" => "bool".into(),
                "urn:oddlang:http:I64" => {
                    let bounds = f
                        .constraints
                        .iter()
                        .find_map(|constraint| match c.constraints[constraint] {
                            Constraint::Bounds { min, max } => Some((min, max)),
                            _ => None,
                        })
                        .unwrap_or((i64::MIN, i64::MAX));
                    format!("Decimal<{{{}_i64}},{{{}_i64}}>", bounds.0, bounds.1)
                }
                _ => record_name(&f.shape)?.into(),
            };
            writeln!(
                source,
                "    #[serde(rename = {:?})]\n    pub r#{}: {},",
                f.wire_name,
                identifier(&f.native_name)?,
                ty
            )
            .unwrap();
        }
        source.push_str("}\n");
    }
    if c.operations.len() != 2 {
        return Err("Axum ledger binding requires one lookup and one transfer operation".into());
    }
    let op = |method| {
        c.operations
            .values()
            .find(|o| o.method == method)
            .ok_or_else(|| format!("missing {method} operation"))
    };
    let get = op("GET")?;
    let post = op("POST")?;
    let prefix = get
        .path
        .strip_suffix("{id}")
        .ok_or("Axum ledger lookup requires a trailing {id} segment")?;
    for (name, value) in [
        ("GET_PATH", get.path.as_str()),
        ("EMPTY_PATH", prefix),
        ("POST_PATH", post.path.as_str()),
        (
            "BEHAVIOR",
            post.behavior
                .as_ref()
                .ok_or("missing transfer behavior")?
                .as_str(),
        ),
    ] {
        writeln!(source, "pub const {name}: &str = {value:?};").unwrap();
    }
    writeln!(
        source,
        "const GET_STATUS:u16={};\nconst POST_STATUS:u16={};",
        get.status, post.status
    )
    .unwrap();
    source.push_str("fn error_status(iri: &str) -> u16 { match iri {\n");
    for (id, status) in &c.errors {
        writeln!(source, "{:?} => {status},", id.as_str()).unwrap();
    }
    source.push_str("_ => 500,\n}}\n");
    source.push_str(include_str!("rust_binding.in"));
    fs::create_dir_all(out)
        .and_then(|_| fs::write(out.join("binding.rs"), source))
        .map_err(|e| e.to_string())
}
