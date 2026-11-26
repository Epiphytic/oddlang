//! Project bounded single-request Unix IPC as a native command handler.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

mod discovery;
mod replies;
mod rust;
use replies::{Declaration, Primitive, ValueType};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    socket_argument: String,
    discovery: Option<discovery::Discovery>,
    transport: Transport,
    #[serde(default)]
    guards: Vec<Guard>,
    request: Vec<RequestField>,
    response: Response,
    errors: Errors,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    framing: Framing,
    peer: Peer,
    deadline_ms: u32,
    maximum_frame_bytes: u32,
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
#[serde(tag = "condition", rename_all = "snake_case", deny_unknown_fields)]
enum Guard {
    FlagFalse {
        argument: String,
        rejected: Diagnostic,
    },
    Absent {
        argument: String,
        rejected: Diagnostic,
    },
    OneOf {
        argument: String,
        values: Vec<String>,
        rejected: Diagnostic,
    },
    PathPresent {
        argument: String,
        rejected: Diagnostic,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestField {
    key: String,
    value: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    id: Option<String>,
    fields: Vec<ResponseField>,
    #[serde(default)]
    declarations: Vec<Declaration>,
    ordered_sequence_minimum: Option<usize>,
    success_field: String,
    success_exit: u8,
    failure_exit: u8,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResponseField {
    id: Option<String>,
    #[serde(default)]
    default: bool,
    key: String,
    kind: ValueType,
    presence: Presence,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Presence {
    Required,
    Nullable,
    OmitNull,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Errors {
    binding: Diagnostic,
    io: Diagnostic,
    peer: Diagnostic,
    frame_size: Diagnostic,
    frame_json: Diagnostic,
    deadline: Diagnostic,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    exit: u8,
    stderr: String,
}

impl Diagnostic {
    fn validate(&self) -> Result<(), String> {
        if self.exit == 0 || self.stderr.is_empty() || self.stderr.chars().any(char::is_control) {
            return Err("invalid local command diagnostic".into());
        }
        Ok(())
    }
}

fn arg_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn unique_keys<'a>(keys: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    keys.into_iter()
        .all(|key| !key.is_empty() && seen.insert(key))
}

impl Graph {
    fn validate(&self) -> Result<(), String> {
        let Transport {
            framing: Framing::U32BeJsonHalfClose,
            peer: Peer::SameEffectiveUid,
            deadline_ms,
            maximum_frame_bytes,
        } = self.transport;
        if self.format != "oddlang-local-command-client-v1"
            || !arg_id(&self.socket_argument)
            || deadline_ms == 0
            || !(1..=16_777_216).contains(&maximum_frame_bytes)
        {
            return Err("invalid local command transport configuration".into());
        }
        for diagnostic in [
            &self.errors.binding,
            &self.errors.io,
            &self.errors.peer,
            &self.errors.frame_size,
            &self.errors.frame_json,
            &self.errors.deadline,
        ] {
            diagnostic.validate()?;
        }
        for guard in &self.guards {
            let (argument, diagnostic) = match guard {
                Guard::FlagFalse { argument, rejected }
                | Guard::Absent { argument, rejected }
                | Guard::PathPresent { argument, rejected } => (argument, rejected),
                Guard::OneOf {
                    argument,
                    values,
                    rejected,
                } => {
                    if values.is_empty() || !unique_keys(values.iter().map(String::as_str)) {
                        return Err("invalid argument choice guard".into());
                    }
                    (argument, rejected)
                }
            };
            if !arg_id(argument) {
                return Err("invalid command argument identity".into());
            }
            diagnostic.validate()?;
        }
        if self.request.is_empty()
            || !unique_keys(self.request.iter().map(|field| field.key.as_str()))
        {
            return Err("invalid request field identity".into());
        }
        if let Some(discovery) = &self.discovery {
            discovery.validate(&self.socket_argument)?;
        }
        replies::validate(&self.response)?;
        if !unique_keys(self.response.fields.iter().map(|field| field.key.as_str()))
            || !self.response.fields.iter().any(|field| {
                field.key == self.response.success_field
                    && matches!(field.kind, ValueType::Primitive(Primitive::Bool))
                    && matches!(field.presence, Presence::Required)
            })
            || self.response.success_exit == self.response.failure_exit
        {
            return Err("invalid response fields or status mapping".into());
        }
        Ok(())
    }

    fn request_json(&self) -> String {
        let fields = self
            .request
            .iter()
            .map(|field| {
                format!(
                    "{}:{}",
                    serde_json::Value::String(field.key.clone()),
                    field.value
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{fields}}}")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    format: String,
    entry: String,
}

#[derive(Clone, Copy)]
pub(crate) enum RequiredArgument {
    Flag,
    OptionalValue,
    String,
    RepeatedString,
    Unsigned64,
    Path,
}

pub(crate) struct Callback {
    pub entry: String,
    pub arguments: Vec<(String, RequiredArgument)>,
}

pub(crate) fn callback(graph: &[u8], binding: &[u8]) -> Result<Callback, String> {
    let (graph, binding) = load(graph, binding)?;
    let mut arguments = vec![(graph.socket_argument, RequiredArgument::Path)];
    if let Some(discovery) = &graph.discovery {
        arguments.extend(discovery.arguments());
    }
    arguments.extend(graph.guards.into_iter().map(|guard| match guard {
        Guard::FlagFalse { argument, .. } => (argument, RequiredArgument::Flag),
        Guard::Absent { argument, .. } => (argument, RequiredArgument::OptionalValue),
        Guard::OneOf { argument, .. } => (argument, RequiredArgument::String),
        Guard::PathPresent { argument, .. } => (argument, RequiredArgument::Path),
    }));
    Ok(Callback {
        entry: binding.entry,
        arguments,
    })
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<(), String> {
    let graph = fs::read(graph).map_err(|error| error.to_string())?;
    let binding = fs::read(binding).map_err(|error| error.to_string())?;
    build_bytes(&graph, &binding, output)
}

pub(crate) fn build_bytes(graph: &[u8], binding: &[u8], output: &Path) -> Result<(), String> {
    let (contract, overlay) = load(graph, binding)?;
    let source = rust::render(&contract, &overlay.entry)?;
    let hash = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let obligations =
        replies::obligations(&contract.response).map(|catalog| format!("{catalog:#}\n"));
    let endpoint_obligations = contract
        .discovery
        .as_ref()
        .map(|discovery| format!("{:#}\n", discovery.obligations()));
    let mut lock = serde_json::json!({
        "format": "oddlang-local-command-client-lock-v1",
        "graphSha256": hash(graph),
        "bindingSha256": hash(binding),
        "sourceSha256": hash(source.as_bytes())
    });
    if let Some(discovery) = &contract.discovery {
        lock["discoverySha256"] =
            hash(&serde_json::to_vec(discovery).map_err(|error| error.to_string())?).into();
    }
    if let Some(catalog) = &endpoint_obligations {
        lock["endpointObligationsSha256"] = hash(catalog.as_bytes()).into();
    }
    if let Some(obligations) = &obligations {
        lock["replyObligationsSha256"] = hash(obligations.as_bytes()).into();
    }
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("client.rs"), source).map_err(|error| error.to_string())?;
    if let Some(catalog) = endpoint_obligations {
        fs::write(stage.path().join("endpoint-obligations.json"), catalog)
            .map_err(|error| error.to_string())?;
    }
    if let Some(obligations) = obligations {
        fs::write(stage.path().join("reply-obligations.json"), obligations)
            .map_err(|error| error.to_string())?;
    }
    fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n"))
        .map_err(|error| error.to_string())?;
    crate::native_package::publish(stage.path(), output)
}

fn load(graph: &[u8], binding: &[u8]) -> Result<(Graph, Binding), String> {
    let contract: Graph = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    contract.validate()?;
    let overlay: Binding = serde_json::from_slice(binding).map_err(|error| error.to_string())?;
    if overlay.format != "oddlang-local-command-client-rust-v1"
        || !overlay
            .entry
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        || !overlay
            .entry
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || overlay.entry == "gen"
        || syn::parse_str::<syn::Ident>(&overlay.entry).is_err()
        || matches!(overlay.entry.as_str(), "run_inner" | "perform" | "exchange")
        || (contract.discovery.is_some()
            && (overlay.entry == "discover_endpoint" || overlay.entry.starts_with("endpoint_")))
    {
        return Err("invalid local command client entry".into());
    }
    Ok((contract, overlay))
}
