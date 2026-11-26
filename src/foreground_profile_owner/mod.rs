//! Project lease-held foreground startup against an existing private profile.

use crate::local_command_client::{Callback, RequiredArgument};
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
    arguments: Arguments,
    profile: Profile,
    runtime: Runtime,
    provider: Provider,
    activation: Activation,
    diagnostics: Diagnostics,
    lifecycle: Option<Lifecycle>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lifecycle {
    storage_profile: String,
    engine: EngineRole,
    excluded_profile_entries: Vec<String>,
    shutdown_ms: u64,
    dial_ms: u64,
    dial_attempts: u8,
    report_version: u8,
    report_fields: ReportFields,
    provider_open_failure: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum EngineRole {
    Absent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportFields {
    schema: String,
    engine_quiesced: String,
    engine_stop_complete: String,
    provider_stop_complete: String,
    runtime_cleanup_complete: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    foreground: String,
    state: String,
    identity: String,
    backend: String,
    backend_choices: BTreeMap<String, String>,
    route: String,
    relay: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    document: String,
    lock: String,
    format: u8,
    maximum_bytes: u32,
    name_limit: usize,
    short_reference_bytes: usize,
    credential_reference_bytes: usize,
    ready: String,
    backend: String,
    fields: ProfileFields,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFields {
    format: String,
    profile_id: String,
    name: String,
    transaction: String,
    phase: String,
    backend: String,
    signing_ref: String,
    data_ref: String,
    device: String,
    public_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    prefix: String,
    binding: String,
    socket: String,
    format: u8,
    fields: BindingFields,
    maximum_path_bytes_exclusive: usize,
    reserved_path_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFields {
    format: String,
    profile_id: String,
    root: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provider {
    kind: ProviderKind,
    directory: String,
    signing_prefix: String,
    data_prefix: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderKind {
    PrivateFileSecp256k1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    kind: ActivationKind,
    route_value: String,
    deadline_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActivationKind {
    LoopbackWebPkiTls,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    scope: String,
    io: String,
    invalid_name: String,
    unsafe_directory: String,
    unsafe_file: String,
    already_running: String,
    missing_profile: String,
    incompatible_format: String,
    invalid_profile: String,
    not_ready: String,
    backend_mismatch: String,
    missing_credential: String,
    invalid_credential: String,
    identity_mismatch: String,
    runtime_collision: String,
    runtime_length: String,
    cleanup: String,
    relay_scope: String,
    plaintext_relay: String,
    relay_failed: String,
    activation_deadline: String,
    provider_unavailable: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    format: String,
    entry: String,
    status_owner: Vec<String>,
    retained_stores: Option<Vec<String>>,
}

impl Graph {
    fn validate(&self, binding: &Binding) -> Result<(), String> {
        match (&self.lifecycle, &binding.retained_stores) {
            (Some(lifecycle), Some(path)) => {
                let fields = &lifecycle.report_fields;
                let EngineRole::Absent = lifecycle.engine;
                let excludes = &lifecycle.excluded_profile_entries;
                if oxigraph::model::NamedNode::new(&lifecycle.storage_profile).is_err()
                    || !binding_path(path)
                    || excludes.is_empty()
                    || !excludes.iter().all(|leaf| filename(leaf))
                    || excludes.iter().collect::<BTreeSet<_>>().len() != excludes.len()
                    || excludes.iter().any(|leaf| {
                        [
                            &self.profile.document,
                            &self.profile.lock,
                            &self.provider.directory,
                        ]
                        .contains(&leaf)
                    })
                    || !(1..=30_000).contains(&lifecycle.shutdown_ms)
                    || !(1..=60_000).contains(&lifecycle.dial_ms)
                    || lifecycle.dial_ms > self.activation.deadline_ms
                    || !(1..=10).contains(&lifecycle.dial_attempts)
                    || lifecycle.report_version == 0
                    || !plain(&lifecycle.provider_open_failure)
                    || !unique([
                        &fields.schema,
                        &fields.engine_quiesced,
                        &fields.engine_stop_complete,
                        &fields.provider_stop_complete,
                        &fields.runtime_cleanup_complete,
                    ])
                {
                    return Err("invalid foreground provider lifecycle".into());
                }
            }
            (None, None) => (),
            _ => {
                return Err(
                    "foreground lifecycle requires an explicit retained store binding".into(),
                );
            }
        }
        let Activation {
            kind: ActivationKind::LoopbackWebPkiTls,
            route_value,
            deadline_ms,
        } = &self.activation;
        if !plain(route_value) || !(1..=300_000).contains(deadline_ms) {
            return Err("invalid foreground activation provider".into());
        }

        if self.format != "oddlang-foreground-profile-owner-v1"
            || binding.format != "oddlang-foreground-profile-owner-rust-v1"
            || !ident(&binding.entry)
            || [
                "start",
                "run",
                "uid",
                "directory",
                "owned_file",
                "credential",
                "checked_profile",
                "lower_hex",
                "check_provider",
                "arguments",
                "matches_scope",
                "activate",
                "relay",
            ]
            .contains(&binding.entry.as_str())
            || !binding_path(&binding.status_owner)
        {
            return Err("invalid foreground owner binding".into());
        }
        if !unique([
            &self.arguments.foreground,
            &self.arguments.state,
            &self.arguments.identity,
            &self.arguments.backend,
            &self.arguments.route,
            &self.arguments.relay,
        ]) || [
            &self.arguments.foreground,
            &self.arguments.state,
            &self.arguments.identity,
            &self.arguments.backend,
            &self.arguments.route,
            &self.arguments.relay,
        ]
        .into_iter()
        .any(|id| !token(id))
            || self.arguments.backend_choices.is_empty()
            || self
                .arguments
                .backend_choices
                .iter()
                .any(|(choice, backend)| !plain(choice) || backend != &self.profile.backend)
        {
            return Err("invalid foreground owner arguments".into());
        }
        let profile = &self.profile;
        let runtime = &self.runtime;
        let Provider {
            kind: ProviderKind::PrivateFileSecp256k1,
            directory,
            signing_prefix,
            data_prefix,
        } = &self.provider;
        if [
            &profile.document,
            &profile.lock,
            &runtime.binding,
            &runtime.socket,
            directory,
        ]
        .into_iter()
        .any(|value| !filename(value))
            || [signing_prefix, data_prefix, &runtime.prefix]
                .into_iter()
                .any(|value| !token(value))
            || !unique([&profile.document, &profile.lock, directory])
            || !unique([&runtime.binding, &runtime.socket])
            || signing_prefix == data_prefix
            || !(1..=65_536).contains(&profile.maximum_bytes)
            || !(1..=128).contains(&profile.name_limit)
            || !(1..=64).contains(&profile.short_reference_bytes)
            || !(1..=64).contains(&profile.credential_reference_bytes)
            || !(2..=4096).contains(&runtime.maximum_path_bytes_exclusive)
            || runtime.reserved_path_bytes < runtime.socket.len() + 1
            || runtime.reserved_path_bytes >= runtime.maximum_path_bytes_exclusive
            || !plain(&profile.ready)
            || !plain(&profile.backend)
        {
            return Err("invalid foreground owner paths or bounds".into());
        }
        let names = &profile.fields;
        if !unique([
            &names.format,
            &names.profile_id,
            &names.name,
            &names.transaction,
            &names.phase,
            &names.backend,
            &names.signing_ref,
            &names.data_ref,
            &names.device,
            &names.public_key,
        ]) || !unique([
            &runtime.fields.format,
            &runtime.fields.profile_id,
            &runtime.fields.root,
        ]) {
            return Err("invalid foreground owner wire field names".into());
        }
        if self
            .diagnostics
            .values()
            .into_iter()
            .any(|(_, message)| !plain(message))
        {
            return Err("invalid foreground owner diagnostic".into());
        }
        Ok(())
    }
}

impl Diagnostics {
    fn values(&self) -> [(&'static str, &String); 22] {
        [
            ("SCOPE", &self.scope),
            ("IO", &self.io),
            ("INVALID_NAME", &self.invalid_name),
            ("UNSAFE_DIRECTORY", &self.unsafe_directory),
            ("UNSAFE_FILE", &self.unsafe_file),
            ("ALREADY_RUNNING", &self.already_running),
            ("MISSING_PROFILE", &self.missing_profile),
            ("INCOMPATIBLE_FORMAT", &self.incompatible_format),
            ("INVALID_PROFILE", &self.invalid_profile),
            ("NOT_READY", &self.not_ready),
            ("BACKEND_MISMATCH", &self.backend_mismatch),
            ("MISSING_CREDENTIAL", &self.missing_credential),
            ("INVALID_CREDENTIAL", &self.invalid_credential),
            ("IDENTITY_MISMATCH", &self.identity_mismatch),
            ("RUNTIME_COLLISION", &self.runtime_collision),
            ("RUNTIME_LENGTH", &self.runtime_length),
            ("CLEANUP", &self.cleanup),
            ("RELAY_SCOPE", &self.relay_scope),
            ("PLAINTEXT_RELAY", &self.plaintext_relay),
            ("RELAY_FAILED", &self.relay_failed),
            ("ACTIVATION_DEADLINE", &self.activation_deadline),
            ("PROVIDER_UNAVAILABLE", &self.provider_unavailable),
        ]
    }
}

fn ident(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && value != "gen"
        && syn::parse_str::<syn::Ident>(value).is_ok()
}
fn binding_path(value: &[String]) -> bool {
    !value.is_empty()
        && value
            .iter()
            .all(|part| part != "gen" && syn::parse_str::<syn::Ident>(part).is_ok())
}
fn plain(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
fn filename(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
fn unique<const N: usize>(values: [&String; N]) -> bool {
    values.into_iter().all(|value| plain(value))
        && values.into_iter().collect::<BTreeSet<_>>().len() == N
}

fn load(graph: &[u8], overlay: &[u8]) -> Result<(Graph, Binding), String> {
    let decode = |error: serde_json::Error| error.to_string();
    let graph: Graph = serde_json::from_slice(graph).map_err(decode)?;
    let binding: Binding = serde_json::from_slice(overlay).map_err(decode)?;
    graph.validate(&binding)?;
    Ok((graph, binding))
}

pub(crate) fn callback(
    graph: &[u8],
    binding: &[u8],
    status_owners: &BTreeSet<Vec<String>>,
    retained_stores: &BTreeMap<Vec<String>, String>,
) -> Result<Callback, String> {
    let (graph, binding) = load(graph, binding)?;
    if !status_owners.contains(&binding.status_owner) {
        return Err("foreground startup requires a linked local status owner".into());
    }
    if let Some(lifecycle) = graph.lifecycle
        && binding
            .retained_stores
            .as_ref()
            .and_then(|path| retained_stores.get(path))
            != Some(&lifecycle.storage_profile)
    {
        return Err(
            "foreground retained store binding differs from its pinned storage profile".into(),
        );
    }
    let args = graph.arguments;
    Ok(Callback {
        local_arguments: None,
        operation: None,
        entry: binding.entry,
        arguments: vec![
            (args.foreground, RequiredArgument::Flag),
            (args.state, RequiredArgument::Path),
            (args.identity, RequiredArgument::String),
            (args.backend, RequiredArgument::String),
            (args.route, RequiredArgument::String),
            (args.relay, RequiredArgument::RepeatedString),
        ],
    })
}

pub(crate) fn build_bytes(graph: &[u8], binding: &[u8], output: &Path) -> Result<(), String> {
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let (model, overlay) = load(graph, binding)?;
    let source = rust::render(&model, &overlay);
    let hash = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let lock = serde_json::json!({
        "format": "oddlang-foreground-profile-owner-lock-v1",
        "graphSha256": hash(graph), "bindingSha256": hash(binding), "sourceSha256": hash(source.as_bytes()),
    });
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("foreground.rs"), source).map_err(|error| error.to_string())?;
    fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n"))
        .map_err(|error| error.to_string())?;
    crate::native_package::publish(stage.path(), output)
}
