//! Project strict bounded ASCII lines and typed captures to native Rust.
mod model;
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
