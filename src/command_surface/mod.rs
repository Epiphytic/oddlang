use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

mod recover;
mod rust;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    profile: String,
    package_name: String,
    version: String,
    root: Command,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_false")]
    external: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    terminal_subparsers: BTreeMap<String, Command>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    gaps: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Command {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    hidden: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    visible_aliases: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    arguments: Vec<Argument>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    groups: Vec<Group>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subcommands: Vec<Command>,
    #[serde(skip_serializing_if = "is_false")]
    subcommand_required: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Argument {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<char>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    visible_aliases: Vec<String>,
    value_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<ValueKind>,
    #[serde(skip_serializing_if = "is_false")]
    required: bool,
    #[serde(skip_serializing_if = "is_false")]
    repeated: bool,
    #[serde(skip_serializing_if = "is_false")]
    global: bool,
    #[serde(skip_serializing_if = "is_false")]
    last: bool,
    #[serde(skip_serializing_if = "is_false")]
    trailing_var_arg: bool,
    #[serde(skip_serializing_if = "is_false")]
    allow_hyphen_values: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    possible_values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    conflicts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    requires: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ValueKind {
    String,
    OsString,
    Path,
    U32,
    U64,
    Usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bounds {
    minimum: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    maximum: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Group {
    id: String,
    #[serde(skip_serializing_if = "is_false")]
    required: bool,
    #[serde(skip_serializing_if = "is_false")]
    multiple: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    arguments: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    root: String,
    package_name: String,
    version: String,
    #[serde(default)]
    gaps: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    executable: String,
    invalid_exit: u8,
    invalid_diagnostic: Option<String>,
    uncovered_exit: u8,
    operations: BTreeMap<String, String>,
    #[serde(default)]
    handlers: BTreeMap<String, Vec<String>>,
}

fn is_false(value: &bool) -> bool {
    !value
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn write(path: &Path, contents: String) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn recover(source: &Path, selection: &Path, output: &Path) -> Result<(), String> {
    let selection = read(selection)?;
    let source = std::fs::read_to_string(source).map_err(|error| error.to_string())?;
    let graph = recover::extract(&source, selection)?;
    let json = serde_json::to_string_pretty(&graph).map_err(|error| error.to_string())?;
    write(output, json + "\n")
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<(), String> {
    let binding: Binding = read(binding)?;
    if !binding.handlers.is_empty() {
        return Err("native command handlers require native-package link validation".into());
    }
    write(output, rust::render(&read(graph)?, &binding)?)
}

pub(crate) fn build_package_component(
    graph: &[u8],
    binding: &[u8],
    output: &Path,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let decode = |error: serde_json::Error| error.to_string();
    let source = rust::render(
        &serde_json::from_slice(graph).map_err(decode)?,
        &serde_json::from_slice(binding).map_err(decode)?,
    )?;
    let hash = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let lock = serde_json::json!({
        "format": "oddlang-command-surface-lock-v1",
        "provider": "clap@4.6.7",
        "graphSha256": hash(graph),
        "bindingSha256": hash(binding),
        "sourceSha256": hash(source.as_bytes()),
    });
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    write(&output.join("cli.rs"), source)?;
    write(&output.join("projection.lock"), format!("{lock:#}\n"))
}

pub(crate) fn validate_callbacks(
    graph: &[u8],
    overlay: &[u8],
    callbacks: &BTreeMap<Vec<String>, &crate::local_command_client::Callback>,
) -> Result<(), String> {
    use crate::local_command_client::RequiredArgument;
    let binding: Binding = serde_json::from_slice(overlay).map_err(|error| error.to_string())?;
    if binding.handlers.is_empty() {
        return Ok(());
    }
    let graph: Graph = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    for (operation, path) in &binding.handlers {
        let callback = callbacks.get(path).ok_or_else(|| {
            format!("unresolved checked native command handler for {operation:?}")
        })?;
        if callback
            .operation
            .as_ref()
            .is_some_and(|id| id != operation)
        {
            return Err(format!(
                "native handler operation identity differs for {operation:?}"
            ));
        }
        for (name, expected) in &callback.arguments {
            let arg = graph
                .root
                .arguments
                .iter()
                .find(|argument| argument.id == *name)
                .ok_or_else(|| {
                    format!("handler {operation:?} needs undeclared root argument {name:?}")
                })?;
            let kind_matches = match expected {
                RequiredArgument::Flag => arg.kind.is_none(),
                RequiredArgument::OptionalValue => {
                    arg.kind.is_some() && !arg.required && arg.default_value.is_none()
                }
                RequiredArgument::String | RequiredArgument::RepeatedString => {
                    matches!(arg.kind, Some(ValueKind::String))
                }
                RequiredArgument::Unsigned64 => matches!(arg.kind, Some(ValueKind::U64)),
                RequiredArgument::Path => matches!(arg.kind, Some(ValueKind::Path)),
            };
            // Clap stores subcommand-local options on a different ArgMatches.
            // Global inheritance prevents both ID and spelling shadowing.
            if arg.repeated != matches!(expected, RequiredArgument::RepeatedString)
                || !kind_matches
                || ((!graph.root.subcommands.is_empty() || graph.external) && !arg.global)
            {
                return Err(format!(
                    "handler {operation:?} has incompatible argument {name:?}"
                ));
            }
        }
    }
    Ok(())
}
