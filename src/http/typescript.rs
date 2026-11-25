//! Emit browser-safe Fetch bindings with separately named native and wire fields.
use super::{Constraint, HttpContract, HttpError, Iri};
use serde_json::{Map, Value, json};
use std::{fmt::Write, fs, path::Path};

fn short(id: &Iri) -> &str {
    id.as_str().rsplit(':').next().unwrap()
}

pub fn emit_typescript(c: &HttpContract, out: &Path) -> Result<(), HttpError> {
    let mut source = String::from("// Generated from the pinned HTTP contract.\n");
    let mut shapes = Map::new();
    for (id, record) in &c.records {
        let name = short(id);
        if !name.starts_with(|c: char| c.is_ascii_alphabetic())
            || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err(format!("unsupported TypeScript record name {name}"));
        }
        writeln!(source, "export interface {name} {{").unwrap();
        let mut fields = Vec::new();
        for field in &record.fields {
            let f = &c.fields[field];
            let kind = short(&f.shape);
            let ty = match kind {
                "I64" => "bigint",
                "Boolean" => "boolean",
                "Text" => "string",
                other => other,
            };
            writeln!(source, "  {}: {ty};", json!(f.native_name)).unwrap();
            let (min, max) = f
                .constraints
                .iter()
                .find_map(|id| match c.constraints[id] {
                    Constraint::Bounds { min, max } => Some((min, max)),
                    _ => None,
                })
                .unwrap_or((i64::MIN, i64::MAX));
            fields.push(json!({"wire":f.wire_name,"name":f.native_name,"kind":kind,"min":min.to_string(),"max":max.to_string()}));
        }
        source.push_str("}\n");
        shapes.insert(name.to_owned(), json!(fields));
    }
    let mut ops = Map::new();
    if c.operations.len() != 2 {
        return Err("Fetch ledger binding requires exactly one lookup and one transfer".into());
    }
    for (id, op) in &c.operations {
        match (
            id.as_str(),
            op.method.as_str(),
            short(&op.request),
            short(&op.response),
        ) {
            ("urn:oddlang:ledger-http:LookupAccount", "GET", "Lookup", "Account")
                if op.path.ends_with("/{id}") => {}
            ("urn:oddlang:ledger-http:TransferFunds", "POST", "Transfer", "Pair") => {}
            _ => return Err(format!("unsupported Fetch ledger operation {id}")),
        }
        let errors: Map<String, Value> = op
            .errors
            .iter()
            .map(|id| (id.to_string(), json!(c.errors[id])))
            .collect();
        ops.insert(op.method.clone(),json!({"method":op.method,"path":op.path,"status":op.status,"request":short(&op.request),"response":short(&op.response),"errors":errors}));
    }
    writeln!(source,"const records: Record<string,Field[]> = {};\nconst operations: Record<string,Operation> = {};",json!(shapes),json!(ops)).unwrap();
    source.push_str(include_str!("typescript_binding.in"));
    fs::create_dir_all(out)
        .and_then(|_| fs::write(out.join("client.ts"), source))
        .map_err(|e| e.to_string())
}
