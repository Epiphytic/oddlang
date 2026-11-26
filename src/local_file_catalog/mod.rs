//! Select one inert local catalog and verify its pinned bytes and ordered assets.
use crate::{
    native_package::{ident, publish},
    wire_schema::{digest, encode, iri},
};
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Graph {
    format: String,
    id: String,
    pub manifest_grammar: String,
    pub file_policy: String,
    fixed_catalog: String,
    prefixes: Vec<String>,
    candidate_name_encoding: String,
    attribute_checks: String,
    suffix: String,
    naming_anchor: String,
    asset_names: Vec<String>,
    pub maximum_catalog_bytes: u32,
    digest: String,
    guards: Vec<String>,
    error: Error,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Error {
    id: String,
    ambiguous: String,
    digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Binding {
    format: String,
    entry: String,
    naming_type: String,
    fixed_variant: String,
    selected_variant: String,
    names: String,
    pub record: Record,
    pub file: File,
    pub error: Vec<String>,
    pub invalid_variant: String,
    pub io_variant: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    pub module: Vec<String>,
    pub r#type: String,
    pub parse: String,
    pub asset_name: String,
    pub assets: String,
    pub name: String,
    pub bytes: String,
    pub mode: String,
    pub digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    pub module: Vec<String>,
    pub read: String,
    pub verify: String,
}

fn basename(name: &str) -> bool {
    !matches!(name, "" | "." | "..")
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn path(parts: &[String], minimum: usize) -> bool {
    parts.len() >= minimum && parts[0] == "crate" && parts[1..].iter().all(|part| ident(part))
}

pub(crate) fn parse(raw: &[u8], native: &[u8]) -> Result<(Graph, Binding)> {
    let graph: Graph = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
    let binding: Binding = serde_json::from_slice(native).map_err(|e| e.to_string())?;
    for id in [
        &graph.id,
        &graph.manifest_grammar,
        &graph.file_policy,
        &graph.error.id,
    ] {
        iri(id)?;
    }
    let record = &binding.record;
    let bindings = [&binding.entry, &binding.naming_type, &binding.names];
    if graph.format != "oddlang-local-file-catalog-v1"
        || graph.maximum_catalog_bytes == 0
        || graph.digest != "sha256"
        || graph.candidate_name_encoding != "utf8"
        || graph.attribute_checks != "fixed-owner-and-mode"
        || graph.guards
            != [
                "unique-name",
                "bounded-read",
                "catalog-digest",
                "parse",
                "canonical-name",
                "ordered-assets",
            ]
        || !basename(&graph.fixed_catalog)
        || !basename(&graph.suffix)
        || graph.prefixes.is_empty()
        || graph.prefixes.iter().any(|value| !basename(value))
        || graph.prefixes.iter().enumerate().any(|(index, prefix)| {
            graph.prefixes[index + 1..]
                .iter()
                .any(|other| other.starts_with(prefix) || prefix.starts_with(other))
        })
        || !graph.asset_names.contains(&graph.naming_anchor)
        || graph.asset_names.iter().any(|name| !basename(name))
        || graph.asset_names.iter().collect::<BTreeSet<_>>().len() != graph.asset_names.len()
        || graph.error.ambiguous == graph.error.digest
        || !ident(&graph.error.ambiguous)
        || !ident(&graph.error.digest)
    {
        return Err("unsupported local file catalog policy".into());
    }
    if binding.format != "oddlang-local-file-catalog-rust-v1"
        || !path(&binding.record.module, 2)
        || !path(&binding.file.module, 2)
        || !path(&binding.error, 2)
        || bindings.into_iter().collect::<BTreeSet<_>>().len() != bindings.len()
        || binding.fixed_variant == binding.selected_variant
        || binding.file.read == binding.file.verify
        || binding.invalid_variant == binding.io_variant
        || bindings
            .into_iter()
            .chain([
                &binding.fixed_variant,
                &binding.selected_variant,
                &record.r#type,
                &record.parse,
                &record.asset_name,
                &record.assets,
                &record.name,
                &record.bytes,
                &record.mode,
                &record.digest,
                &binding.file.read,
                &binding.file.verify,
                &binding.invalid_variant,
                &binding.io_variant,
            ])
            .any(|name| !ident(name))
    {
        return Err("invalid local file catalog Rust binding".into());
    }
    Ok((graph, binding))
}

impl Graph {
    pub(crate) fn asset_names(&self) -> &[String] {
        &self.asset_names
    }
    pub(crate) fn error_id(&self) -> &str {
        &self.error.id
    }
}

impl Binding {
    pub(crate) fn exports(&self) -> [&str; 3] {
        [&self.entry, &self.naming_type, &self.names]
    }
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    let raw = fs::read(graph).map_err(|e| e.to_string())?;
    let native = fs::read(overlay).map_err(|e| e.to_string())?;
    let (graph, binding) = parse(&raw, &native)?;
    let record = &binding.record;
    let source = [
        ("@ENTRY@", binding.entry),
        ("@NAMING@", binding.naming_type),
        ("@FIXED@", binding.fixed_variant),
        ("@SELECTED@", binding.selected_variant),
        ("@NAMES@", binding.names),
        ("@ASSET_NAMES@", format!("{:?}", graph.asset_names)),
        ("@ASSET_COUNT@", graph.asset_names.len().to_string()),
        (
            "@MANIFEST@",
            format!("{}::{}", record.module.join("::"), record.r#type),
        ),
        ("@PARSE@", record.parse.clone()),
        ("@NAMER@", record.asset_name.clone()),
        ("@ASSETS@", record.assets.clone()),
        ("@NAME@", record.name.clone()),
        ("@BYTES@", record.bytes.clone()),
        ("@MODE@", record.mode.clone()),
        ("@DIGEST@", record.digest.clone()),
        (
            "@READ@",
            format!("{}::{}", binding.file.module.join("::"), binding.file.read),
        ),
        (
            "@VERIFY@",
            format!(
                "{}::{}",
                binding.file.module.join("::"),
                binding.file.verify
            ),
        ),
        ("@ERROR@", binding.error.join("::")),
        ("@INVALID@", binding.invalid_variant),
        ("@IO@", binding.io_variant),
        ("@LIMIT@", graph.maximum_catalog_bytes.to_string()),
        ("@FIXED_NAME@", format!("{:?}", graph.fixed_catalog)),
        ("@ANCHOR@", format!("{:?}", graph.naming_anchor)),
        ("@PREFIXES@", format!("{:?}", graph.prefixes)),
        ("@SUFFIX@", format!("{:?}", graph.suffix)),
        ("@BAD_PATH@", graph.error.ambiguous),
        ("@BAD_DIGEST@", graph.error.digest),
    ]
    .into_iter()
    .fold(
        include_str!("catalog.rs.txt").to_owned(),
        |text, (token, value)| text.replace(token, &value),
    );
    let seal = encode(&serde_json::json!({
        "format": "oddlang-local-file-catalog-rust-lock-v1",
        "graphSha256": digest(&raw), "overlaySha256": digest(&native), "sourceSha256": digest(source.as_bytes()),
    }))?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("catalog.rs"), source).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("projection.lock"), seal).map_err(|e| e.to_string())?;
    publish(stage.path(), output)
}
