//! Project source-free URL-set normalization to native Rust.
mod render;
mod schema;

use schema::{load_graph, load_overlay, sha256};
use std::{fs, path::Path};

pub fn digest(graph: &Path) -> Result<String, String> {
    sha256(&load_graph(graph)?)
}

pub fn overlay_digest(overlay: &Path) -> Result<String, String> {
    sha256(&load_overlay(overlay)?)
}

pub fn build(
    graph: &Path,
    graph_pin: &str,
    overlay: Option<(&Path, &str)>,
    output: &Path,
) -> Result<(), String> {
    let graph = load_graph(graph)?;
    if sha256(&graph)? != graph_pin {
        return Err("endpoint-set graph digest mismatch".into());
    }
    let binding = overlay
        .map(|(path, pin)| -> Result<_, String> {
            let value = load_overlay(path)?;
            if sha256(&value)? != pin {
                return Err("endpoint-set overlay digest mismatch".into());
            }
            value.validate_bindings(&graph)?;
            Ok((value, pin))
        })
        .transpose()?;

    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage =
        tempfile::tempdir_in(parent).map_err(|err| format!("stage endpoint-set output: {err}"))?;
    let file = "endpoint_set.rs";
    let source = render::rust(&graph, binding.as_ref().map(|(overlay, _)| overlay));
    let lock = serde_json::json!({
        "vocabulary": schema::VOCABULARY,
        "contract": graph.id,
        "graphSha256": graph_pin,
        "overlaySha256": binding.as_ref().map(|(_, pin)| pin),
        "target": "rust",
        "providerHint": graph.provider_hint,
        "source": file,
    });
    fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n"))
        .and_then(|()| fs::write(stage.path().join(file), source))
        .map_err(|err| format!("write endpoint-set output: {err}"))?;
    fs::rename(stage.path(), output).map_err(|err| format!("publish endpoint-set output: {err}"))
}
