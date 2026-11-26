//! Source-free bounded byte storage and ordered transactional operations.
mod render;
mod schema;

use schema::{Graph, Result, RustOverlay};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn digest(path: &Path) -> Result<String> {
    sha256(&load(path)?)
}

pub fn overlay_digest(path: &Path) -> Result<String> {
    sha256(&load_overlay(path)?)
}

pub fn build(
    graph: &Path,
    graph_pin: &str,
    overlay: &Path,
    overlay_pin: &str,
    output: &Path,
) -> Result<()> {
    let graph = load(graph)?;
    if sha256(&graph)? != graph_pin {
        return Err("sequence-journal graph digest mismatch".into());
    }
    let overlay = load_overlay(overlay)?;
    if sha256(&overlay)? != overlay_pin {
        return Err("sequence-journal overlay digest mismatch".into());
    }
    overlay.validate_bindings(&graph)?;
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage =
        tempfile::tempdir_in(parent).map_err(|err| format!("stage journal output: {err}"))?;
    let source = render::rust(&graph, &overlay);
    let lock = serde_json::json!({
        "format": schema::FORMAT,
        "graphSha256": graph_pin,
        "overlaySha256": overlay_pin,
        "target": "rust",
        "source": "sequence_journal.rs",
        "sourceSha256": hash(source.as_bytes()),
        "receiptDigest": "sha256",
    });
    fs::write(stage.path().join("sequence_journal.rs"), source)
        .and_then(|()| fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n")))
        .map_err(|err| format!("write journal output: {err}"))?;
    fs::rename(stage.path(), output).map_err(|err| format!("publish journal output: {err}"))
}

fn load(path: &Path) -> Result<Graph> {
    parse::<Graph>(path, "sequence-journal graph")?.validate()
}

fn load_overlay(path: &Path) -> Result<RustOverlay> {
    let overlay: RustOverlay = parse(path, "sequence-journal overlay")?;
    overlay.validate()?;
    Ok(overlay)
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path, context: &str) -> Result<T> {
    let bytes = fs::read(path).map_err(|err| format!("read {context}: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("invalid {context}: {err}"))
}

fn sha256<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|err| format!("encode journal pin: {err}"))?;
    Ok(hash(&bytes))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
