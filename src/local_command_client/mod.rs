//! Project bounded single-request Unix IPC as a native command handler.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

mod discovery;
mod replies;
mod rust;
mod workflow;
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
    request_id: Option<String>,
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

#[derive(Deserialize, Serialize)]
#[serde(tag = "condition", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Guard {
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
    id: Option<String>,
    key: String,
    #[serde(default, deserialize_with = "present")]
    value: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "present")]
    argument: Option<RequestArgument>,
}

// Distinguish an explicitly supplied JSON null from an absent field.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RequestArgument {
    id: String,
    kind: RequestArgumentKind,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RequestArgumentKind {
    Flag,
    String,
    RepeatedString,
    Unsigned64,
}

impl RequestArgumentKind {
    fn minimum_json(self) -> &'static str {
        match self {
            Self::Flag => "true",
            Self::String => r#""""#,
            Self::RepeatedString => "[]",
            Self::Unsigned64 => "0",
        }
    }

    fn representation(self) -> serde_json::Value {
        match self {
            Self::Flag => serde_json::json!({"type": "boolean", "absent": false}),
            Self::String => serde_json::json!({"type": "string", "absent": "reject"}),
            Self::RepeatedString => serde_json::json!({
                "type": "array", "itemType": "string", "order": "argumentOrder", "absent": []
            }),
            Self::Unsigned64 => serde_json::json!({
                "type": "integer", "minimumDecimal": "0", "maximumDecimal": u64::MAX.to_string(),
                "absent": "reject"
            }),
        }
    }
}

impl RequestArgument {
    fn requirement(&self) -> (String, RequiredArgument) {
        let kind = match self.kind {
            RequestArgumentKind::Flag => RequiredArgument::Flag,
            RequestArgumentKind::String => RequiredArgument::String,
            RequestArgumentKind::RepeatedString => RequiredArgument::RepeatedString,
            RequestArgumentKind::Unsigned64 => RequiredArgument::Unsigned64,
        };
        (self.id.clone(), kind)
    }
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
pub(crate) struct Diagnostic {
    pub(crate) exit: u8,
    pub(crate) stderr: String,
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

impl Guard {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let (argument, diagnostic) = match self {
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
        Ok(())
    }
    pub(crate) fn requirement(&self) -> (String, RequiredArgument) {
        match self {
            Guard::FlagFalse { argument, .. } => (argument.clone(), RequiredArgument::Flag),
            Guard::Absent { argument, .. } => (argument.clone(), RequiredArgument::OptionalValue),
            Guard::OneOf { argument, .. } => (argument.clone(), RequiredArgument::String),
            Guard::PathPresent { argument, .. } => (argument.clone(), RequiredArgument::Path),
        }
    }
}
fn unique_keys<'a>(keys: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    keys.into_iter()
        .all(|key| !key.is_empty() && seen.insert(key))
}

fn contains_fractional_literal(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Number(number) => !number.is_i64() && !number.is_u64(),
        Value::Array(values) => values.iter().any(contains_fractional_literal),
        Value::Object(fields) => fields.values().any(contains_fractional_literal),
        _ => false,
    }
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
            guard.validate()?;
        }
        if self.request.is_empty()
            || !unique_keys(self.request.iter().map(|field| field.key.as_str()))
            || self
                .request_arguments()
                .any(|argument| !arg_id(&argument.id))
        {
            return Err("invalid request field identity".into());
        }
        if self
            .request
            .iter()
            .any(|field| field.value.is_some() == field.argument.is_some())
        {
            return Err("request field needs exactly one literal value or argument".into());
        }
        if self.request_id.is_some()
            || self
                .request
                .iter()
                .any(|field| field.id.is_some() || field.argument.is_some())
        {
            let mut seen = BTreeSet::new();
            for id in std::iter::once(self.request_id.as_deref())
                .chain(self.request.iter().map(|field| field.id.as_deref()))
            {
                let id = id.ok_or("missing request declaration identity")?;
                oxigraph::model::NamedNode::new(id)
                    .map_err(|_| format!("invalid request declaration identity {id}"))?;
                if !seen.insert(id) {
                    return Err(format!("duplicate request declaration identity {id}"));
                }
            }
            if self
                .request
                .iter()
                .filter_map(|field| field.value.as_ref())
                .any(contains_fractional_literal)
            {
                return Err(
                    "identified request literals require signed or unsigned integer JSON numbers"
                        .into(),
                );
            }
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

    fn request_arguments(&self) -> impl Iterator<Item = &RequestArgument> {
        self.request
            .iter()
            .filter_map(|field| field.argument.as_ref())
    }

    fn minimum_request_json(&self) -> String {
        use serde_json::Value;
        let fields = self
            .request
            .iter()
            .map(|field| {
                let key = Value::String(field.key.clone());
                if let Some(argument) = &field.argument {
                    format!("{key}:{}", argument.kind.minimum_json())
                } else {
                    let value = field.value.as_ref().expect("validated literal field");
                    format!("{key}:{value}")
                }
            })
            .collect::<Vec<_>>();
        format!("{{{}}}", fields.join(","))
    }

    fn request_obligations(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let id = self.request_id.as_ref()?;
        let mut rows = vec![json!({
            "declaration": id,
            "role": "request-envelope",
            "framing": "u32_be_json_half_close",
            "maximumFrameBytes": self.transport.maximum_frame_bytes,
            "objectOrder": "declarationOrder",
            "encoding": "compactUtf8Json",
            "literalObjectOrder": "lexicographicUtf8Keys",
            "literalNumberEncoding": "signedOrUnsignedDecimalInteger",
            "argumentSource": "explicitCommandLine",
            "stringEscaping": {
                "controlRange": "U+0000-U+001F",
                "quote": "backslash",
                "reverseSolidus": "backslash",
                "shortControls": ["b", "t", "n", "f", "r"],
                "otherControls": "lowercase-u00xx",
                "nonAscii": "rawUtf8",
                "solidus": "raw",
            },
        })];
        rows.extend(self.request.iter().enumerate().map(|(position, field)| {
            let mut row = json!({
                "declaration": field.id, "role": "request-field", "position": position, "key": field.key,
            });
            if let Some(argument) = &field.argument {
                row["argument"] = json!(argument);
                row["representation"] = argument.kind.representation();
            } else {
                row["value"] = json!(field.value);
            }
            row
        }));
        Some(json!({
            "format": "oddlang-local-command-request-obligations-v1",
            "qualification": "independent-witness-required",
            "obligations": rows,
        }))
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
    pub local_arguments: Option<(String, Vec<(String, RequiredArgument)>)>,
    pub operation: Option<String>,
    pub entry: String,
    pub arguments: Vec<(String, RequiredArgument)>,
}

pub(crate) fn callback(graph: &[u8], binding: &[u8]) -> Result<Callback, String> {
    if workflow::matches_graph(graph) {
        return workflow::callback(graph, binding);
    }
    let (graph, binding) = load(graph, binding)?;
    let mut arguments = vec![(graph.socket_argument.clone(), RequiredArgument::Path)];
    if let Some(discovery) = &graph.discovery {
        arguments.extend(discovery.arguments());
    }
    arguments.extend(graph.guards.iter().map(Guard::requirement));
    arguments.extend(graph.request_arguments().map(RequestArgument::requirement));
    Ok(Callback {
        local_arguments: None,
        operation: None,
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
    if workflow::matches_graph(graph) {
        return workflow::build(graph, binding, output);
    }
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
    let request_obligations = contract
        .request_obligations()
        .map(|catalog| format!("{catalog:#}\n"));
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
    if let Some(catalog) = &request_obligations {
        lock["requestObligationsSha256"] = hash(catalog.as_bytes()).into();
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
    if let Some(catalog) = request_obligations {
        fs::write(stage.path().join("request-obligations.json"), catalog)
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
        || !valid_entry(&overlay.entry)
        || (overlay.entry == "request_body" && contract.request_arguments().next().is_some())
        || (contract.discovery.is_some()
            && (overlay.entry == "discover_endpoint" || overlay.entry.starts_with("endpoint_")))
    {
        return Err("invalid local command client entry".into());
    }
    Ok((contract, overlay))
}

fn valid_entry(entry: &str) -> bool {
    entry
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase())
        && entry
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && entry != "gen"
        && syn::parse_str::<syn::Ident>(entry).is_ok()
        && !matches!(entry, "run_inner" | "perform" | "exchange")
}
