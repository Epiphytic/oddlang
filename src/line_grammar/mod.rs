//! Project strict bounded ASCII lines and typed captures to native Rust.
mod constraints;
mod model;
mod records;
mod rust;

use crate::{
    native_package::publish,
    wire_schema::{digest, encode, json},
};
use model::{Binding, Graph, Result};
use serde_json::json;
use std::{fs, path::Path};

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<()> {
    let graph: Graph = json(graph)?;
    let binding: Binding = json(binding)?;
    let source = rust::render(&graph, &binding)?;
    let lock = encode(&json!({
        "format": "oddlang-line-grammar-projection-v1",
        "graphSha256": digest(&encode(&graph)?),
        "overlaySha256": digest(&encode(&binding)?),
        "sourceSha256": digest(source.as_bytes()),
    }))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("lines.rs"), source).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("projection.lock"), lock).map_err(|e| e.to_string())?;
    publish(stage.path(), output)
}

pub(crate) fn error_link(binding: &[u8]) -> Result<Option<records::NativeError>> {
    let parsed = serde_json::from_slice(binding).map_err(|e| e.to_string())?;
    Ok(match parsed {
        Binding::Records(record) => Some(record.error),
        Binding::Captures(_) => None,
    })
}

pub(crate) fn validate_catalog_record(
    grammar: &[u8],
    native: &[u8],
    catalog: &crate::local_file_catalog::Graph,
    binding: &crate::local_file_catalog::Binding,
) -> Result<()> {
    let graph: Graph = serde_json::from_slice(grammar).map_err(|error| error.to_string())?;
    let projected: Binding = serde_json::from_slice(native).map_err(|error| error.to_string())?;
    graph.validate(&projected)?;
    let (Some(result), Binding::Records(record)) = (&graph.record_result, &projected) else {
        return Err("local file catalog needs one typed line record".into());
    };
    if graph.id != catalog.manifest_grammar
        || graph.max_bytes != catalog.maximum_catalog_bytes
        || record.error.id != catalog.error_id()
        || record.error.r#type != binding.error
        || record.error.variant != binding.invalid_variant
    {
        return Err("local file catalog grammar or error role differs".into());
    }
    result.validate_catalog_roles(&graph, record, &binding.record, catalog.asset_names())
}
