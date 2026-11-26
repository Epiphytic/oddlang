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
    external: bool,
    #[serde(default)]
    gaps: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Command {
    name: String,
    help: Option<String>,
    hidden: bool,
    aliases: Vec<String>,
    visible_aliases: Vec<String>,
    arguments: Vec<Argument>,
    groups: Vec<Group>,
    subcommands: Vec<Command>,
    subcommand_required: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Argument {
    id: String,
    help: Option<String>,
    long: Option<String>,
    short: Option<char>,
    aliases: Vec<String>,
    visible_aliases: Vec<String>,
    value_name: String,
    kind: Option<ValueKind>,
    required: bool,
    repeated: bool,
    global: bool,
    last: bool,
    default_value: Option<String>,
    possible_values: Vec<String>,
    bounds: Option<Bounds>,
    conflicts: Vec<String>,
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
    maximum: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Group {
    id: String,
    required: bool,
    multiple: bool,
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
    write(output, rust::render(&read(graph)?, &read(binding)?)?)
}
