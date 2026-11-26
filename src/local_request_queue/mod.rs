//! Project a scoped durable local queue into an existing native connection.

use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    account_bytes: usize,
    room_bytes: usize,
    id_bytes: usize,
    maximum_frame_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Overlay {
    format: String,
    storage: String,
}

pub(crate) fn build_bytes(
    graph_bytes: &[u8],
    overlay_bytes: &[u8],
    output: &Path,
) -> Result<(), String> {
    let graph: Graph = serde_json::from_slice(graph_bytes)
        .map_err(|error| format!("local request queue graph: {error}"))?;
    let overlay: Overlay = serde_json::from_slice(overlay_bytes)
        .map_err(|error| format!("local request queue overlay: {error}"))?;
    if graph.format != "oddlang-local-request-queue-v1"
        || graph
            .id
            .split_once(':')
            .is_none_or(|(scheme, rest)| scheme.is_empty() || rest.is_empty())
        || !graph.id.bytes().all(|byte| (b'!'..=b'~').contains(&byte))
        || graph.id.len() > 255
        || [graph.account_bytes, graph.room_bytes, graph.id_bytes]
            .iter()
            .any(|width| !(1..=64).contains(width))
        || !(1..=1_048_576).contains(&graph.maximum_frame_bytes)
    {
        return Err("invalid local request queue graph".into());
    }
    if overlay.format != "oddlang-local-request-queue-rust-v1" || overlay.storage != "sqlite" {
        return Err("invalid local request queue overlay".into());
    }
    let source = format!(
        "const CONTRACT_ID: &str = {:?};\n\
         const ACCOUNT_BYTES: i64 = {};\n\
         const ROOM_BYTES: i64 = {};\n\
         const ID_BYTES: i64 = {};\n\
         const MAX_FRAME_BYTES: i64 = {};\n{}",
        graph.id,
        graph.account_bytes,
        graph.room_bytes,
        graph.id_bytes,
        graph.maximum_frame_bytes,
        include_str!("runtime.rs.in")
    );
    let digest = crate::native_package::digest;
    let lock = serde_json::json!({
        "format": "oddlang-local-request-queue-lock-v1",
        "graphSha256": digest(graph_bytes),
        "bindingSha256": digest(overlay_bytes),
        "sourceSha256": digest(source.as_bytes()),
    });
    let stage = tempfile::tempdir_in(output.parent().unwrap_or(Path::new(".")))
        .map_err(|error| error.to_string())?;
    fs::write(stage.path().join("queue.rs"), source).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n"))
        .map_err(|error| error.to_string())?;
    crate::native_package::publish(stage.path(), output)
}
