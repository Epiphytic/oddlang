//! Render a read-only owner snapshot and a private, bounded Unix listener.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod rust;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    request: Request,
    reply: Reply,
    listener: Listener,
    lifecycle: Lifecycle,
    diagnostics: Diagnostics,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    ok: String,
    result: String,
    error: String,
    state: String,
    invalid_request: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listener {
    framing: Framing,
    peer: Peer,
    maximum_frame_bytes: u32,
    maximum_clients: usize,
    maximum_path_bytes_exclusive: usize,
    read_deadline_ms: u64,
    write_deadline_ms: u64,
    accept_retry_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Framing {
    U32BeJsonHalfClose,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Peer {
    SameEffectiveUid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lifecycle {
    bound: String,
    stopping: String,
    #[serde(default)]
    transitions: Vec<Transition>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    from: String,
    to: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    io: String,
    private_parent: String,
    path: String,
    occupied: String,
    invalid_transition: String,
    cleanup: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    format: String,
    executable: String,
    socket_option: String,
    #[serde(default)]
    routed_requests: bool,
}

fn validate(graph: &Graph, binding: &Binding) -> Result<(), String> {
    if graph.format != "oddlang-local-status-owner-v1"
        || binding.format != "oddlang-local-status-owner-rust-v1"
        || !label(&binding.executable)
        || !label(&binding.socket_option)
        || matches!(binding.socket_option.as_str(), "help" | "version")
    {
        return Err("invalid local status graph or Rust binding".into());
    }
    let Listener {
        framing: Framing::U32BeJsonHalfClose,
        peer: Peer::SameEffectiveUid,
        maximum_frame_bytes,
        maximum_clients,
        maximum_path_bytes_exclusive,
        read_deadline_ms,
        write_deadline_ms,
        accept_retry_ms,
    } = graph.listener;
    if !(1..=16_777_216).contains(&maximum_frame_bytes)
        || !(1..=1024).contains(&maximum_clients)
        || !(2..=4096).contains(&maximum_path_bytes_exclusive)
        || [read_deadline_ms, write_deadline_ms, accept_retry_ms]
            .into_iter()
            .any(|ms| !(1..=86_400_000).contains(&ms))
    {
        return Err("invalid local status listener bounds".into());
    }
    let Reply {
        ok,
        result,
        error,
        state,
        invalid_request,
    } = &graph.reply;
    if [ok, result, error]
        .into_iter()
        .collect::<BTreeSet<_>>()
        .len()
        != 3
        || [
            ok,
            result,
            error,
            state,
            invalid_request,
            &graph.request.key,
            &graph.request.value,
        ]
        .into_iter()
        .any(|value| !plain(value))
    {
        return Err("invalid local status wire names".into());
    }
    let Lifecycle {
        bound,
        stopping,
        transitions,
    } = &graph.lifecycle;
    let mut edges = BTreeSet::new();
    if !label(bound)
        || !label(stopping)
        || bound == stopping
        || transitions.iter().any(|edge| {
            !label(&edge.from)
                || !label(&edge.to)
                || edge.from == edge.to
                || &edge.from == stopping
                || &edge.to == stopping
                || !edges.insert((&edge.from, &edge.to))
        })
    {
        return Err("invalid local status transition".into());
    }
    let mut reachable = BTreeSet::from([bound]);
    let mut unvisited = vec![bound];
    let mut from = BTreeMap::<&String, Vec<&String>>::new();
    for (source, destination) in &edges {
        from.entry(source).or_default().push(destination);
    }
    while let Some(state) = unvisited.pop() {
        for successor in from.get(state).into_iter().flatten() {
            if reachable.insert(successor) {
                unvisited.push(successor);
            }
        }
    }
    if edges.iter().any(|(state, _)| !reachable.contains(state)) {
        return Err("unreachable local status transition".into());
    }
    let bounds = [
        serde_json::json!({ &graph.request.key: &graph.request.value }),
        serde_json::json!({ ok: false, result: null, error: invalid_request }),
    ]
    .into_iter()
    .chain(
        reachable
            .into_iter()
            .chain([stopping])
            .map(|phase| serde_json::json!({ ok: true, result: { state: phase }, error: null })),
    );
    if bounds
        .into_iter()
        .any(|body| body.to_string().len() > maximum_frame_bytes as usize)
    {
        return Err("declared local status frame exceeds listener bound".into());
    }
    let Diagnostics {
        io,
        private_parent,
        path,
        occupied,
        invalid_transition,
        cleanup,
    } = &graph.diagnostics;
    if [
        io,
        private_parent,
        path,
        occupied,
        invalid_transition,
        cleanup,
    ]
    .into_iter()
    .any(|value| !plain(value))
    {
        return Err("invalid local status diagnostic".into());
    }
    Ok(())
}

fn plain(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}

fn label(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<(), String> {
    let read = |path: &Path| fs::read(path).map_err(|error| format!("{}: {error}", path.display()));
    build_bytes(&read(graph)?, &read(binding)?, output)
}

pub(crate) fn build_bytes(graph: &[u8], binding: &[u8], output: &Path) -> Result<(), String> {
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let decode = |error: serde_json::Error| error.to_string();
    let model = serde_json::from_slice(graph).map_err(decode)?;
    let overlay = serde_json::from_slice(binding).map_err(decode)?;
    validate(&model, &overlay)?;
    let source = rust::render(&model, &overlay);
    let hash = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let lock = serde_json::json!({
        "format": "oddlang-local-status-owner-lock-v1",
        "graphSha256": hash(graph),
        "bindingSha256": hash(binding),
        "sourceSha256": hash(source.as_bytes()),
    });
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("owner.rs"), source).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n"))
        .map_err(|error| error.to_string())?;
    crate::native_package::publish(stage.path(), output)
}
