//! Offline loading of authored and pinned HTTP contracts.
mod model;
use crate::contracts::{self, Pin, contained, normal, verified};
pub use model::{Constraint, Field, HttpContract, Iri, Observation, Operation, Record, Shape};
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{
        NamedOrBlankNode, Term,
        vocab::{rdfs, xsd},
    },
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub type HttpError = String;
pub const H: &str = "urn:oddlang:http:";
pub(crate) fn version_ok(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    pins: Vec<Pin>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    identity: String,
    version: String,
    entry: String,
    rule_set_version: u32,
    dependencies: Vec<Pin>,
    files: BTreeMap<String, String>,
}
fn pins(entries: Vec<Pin>) -> Result<BTreeMap<String, Pin>, String> {
    let mut result = BTreeMap::new();
    for pin in entries {
        Iri::new(&pin.identity)?;
        if !version_ok(&pin.version)
            || pin.manifest_sha256.len() != 64
            || !pin
                .manifest_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(format!("invalid pin for {}", pin.identity));
        }
        if let Some(old) = result.insert(pin.identity.clone(), pin.clone())
            && old != pin
        {
            return Err(format!("conflicting pins for {}", pin.identity));
        }
    }
    Ok(result)
}
fn lock(path: &Path) -> Result<BTreeMap<String, Pin>, String> {
    let input = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    pins(
        serde_json::from_slice::<Lock>(&input)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .pins,
    )
}

fn read_model(
    entry: &str,
    deps: BTreeMap<String, Pin>,
    cache: &Path,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<HttpContract, String> {
    if !normal(entry) {
        return Err(format!("invalid entry {entry}"));
    }
    let mut symbols = BTreeMap::new();
    for pin in deps.values() {
        let release = contracts::resolve_pin(pin, cache)?;
        for (iri, operation) in release.model.operations {
            symbols.insert(Iri::new(iri)?, operation);
        }
    }
    let mut nodes = model::Nodes::new();
    let mut files = BTreeMap::new();
    let mut scenarios = BTreeSet::new();
    let mut pending = BTreeSet::from([entry.to_owned()]);
    let mut problems = BTreeSet::new();
    while let Some(path) = pending.pop_first() {
        if files.contains_key(&path) {
            continue;
        }
        let bytes = read(&path)?;
        for quad in RdfParser::from_format(RdfFormat::Turtle)
            .rename_blank_nodes()
            .for_reader(bytes.as_slice())
        {
            let q = quad.map_err(|e| format!("{path}: {e}"))?;
            let key = q.predicate.as_str();
            if key == rdfs::LABEL.as_str() || key == rdfs::COMMENT.as_str() {
                continue;
            }
            let NamedOrBlankNode::NamedNode(id) = q.subject else {
                problems.insert(format!(
                    "anonymous subject {key}: expected named declaration"
                ));
                continue;
            };
            let id = Iri::new(id.into_string())?;
            if matches!(
                key,
                "urn:oddlang:http:localImport" | "urn:oddlang:http:scenarioFile"
            ) {
                match &q.object {
                    Term::Literal(v) if v.datatype() == xsd::STRING && normal(v.value()) => {
                        let name = Path::new(&path)
                            .parent()
                            .unwrap_or(Path::new(""))
                            .join(v.value())
                            .to_str()
                            .unwrap()
                            .to_owned();
                        if key.ends_with("localImport") {
                            pending.insert(name);
                        } else {
                            scenarios.insert(name);
                        }
                    }
                    _ => {
                        problems.insert(format!("{id} {key}: expected normal relative path"));
                    }
                }
            }
            nodes
                .entry(id)
                .or_default()
                .entry(key.to_owned())
                .or_default()
                .insert(q.object);
        }
        files.insert(path, bytes);
    }
    if let Some(e) = problems.pop_first() {
        return Err(e);
    }
    for file in &scenarios {
        if files.contains_key(file) {
            return Err(format!("scenario also declared as ontology: {file}"));
        }
        files.insert(file.clone(), read(file)?);
    }
    let mut model = model::parse(nodes, deps.into_values().collect(), &symbols)?;
    model.source_files = files;
    model.entry = entry.to_owned();
    if model.rule_set_version == 2 && !scenarios.is_empty() {
        return Err("codec contracts do not import authored scenarios".into());
    }
    model.scenario_files = scenarios;
    Ok(model)
}
pub fn load_source(
    root: &Path,
    dependency_lock: &Path,
    cache: &Path,
) -> Result<HttpContract, HttpError> {
    read_model("domain.ttl", lock(dependency_lock)?, cache, |name| {
        fs::read(contained(root, name)?).map_err(|e| format!("{name}: {e}"))
    })
}
pub fn load(
    project: &Path,
    workspace_lock: &Path,
    cache: &Path,
) -> Result<HttpContract, HttpError> {
    let mut models = load_workspace(project, workspace_lock, cache)?;
    if models.len() != 1 {
        return Err("expected one HTTP project import".into());
    }
    Ok(models.pop_first().unwrap().1)
}

pub fn load_workspace(
    project: &Path,
    workspace_lock: &Path,
    cache: &Path,
) -> Result<BTreeMap<Iri, HttpContract>, HttpError> {
    let workspace = lock(workspace_lock)?;
    let bytes = fs::read(project).map_err(|e| format!("project: {e}"))?;
    let mut imports = BTreeSet::new();
    for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(bytes.as_slice()) {
        let q = quad.map_err(|e| format!("project: {e}"))?;
        if q.predicate.as_str() == "urn:oddlang:core:imports" {
            let Term::NamedNode(node) = q.object else {
                return Err("project import must be a named IRI".into());
            };
            imports.insert(node.into_string());
        }
    }
    if imports.is_empty() {
        return Err("expected HTTP project imports".into());
    }
    let mut result = BTreeMap::new();
    let mut dependencies = workspace.clone();
    for import in imports {
        let model = load_import(&import, &workspace, cache)?;
        for dep in &model.dependencies {
            if dependencies
                .insert(dep.identity.clone(), dep.clone())
                .is_some_and(|existing| existing != *dep)
            {
                return Err(format!("conflicting dependency pin for {}", dep.identity));
            }
        }
        result.insert(model.identity.clone(), model);
    }
    Ok(result)
}

fn load_import(
    import: &str,
    workspace: &BTreeMap<String, Pin>,
    cache: &Path,
) -> Result<HttpContract, HttpError> {
    let mut candidates = workspace
        .values()
        .filter(|p| p.identity == import || format!("{}:{}", p.identity, p.version) == import);
    let pin = candidates
        .next()
        .ok_or_else(|| format!("unresolved import {import}"))?;
    if candidates.next().is_some() {
        return Err(format!("ambiguous project import {import}"));
    }
    let identity_hash: String = Sha256::digest(pin.identity.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let root = contained(cache, &format!("http/{identity_hash}/{}", pin.version))?;
    let manifest: Manifest =
        serde_json::from_slice(&verified(&root, "manifest.json", &pin.manifest_sha256)?)
            .map_err(|e| format!("manifest.json: {e}"))?;
    if manifest.schema_version != 1
        || manifest.identity != pin.identity
        || manifest.version != pin.version
        || !matches!(manifest.rule_set_version, 1 | 2)
    {
        return Err("HTTP manifest disagrees with pin or unsupported schema/rule set".into());
    }
    let deps = pins(manifest.dependencies)?;
    for (id, dep) in &deps {
        if workspace.get(id).is_some_and(|p| p != dep) {
            return Err(format!("conflicting dependency pin for {id}"));
        }
    }
    let mut files = BTreeMap::new();
    for (path, digest) in manifest.files {
        let bytes = verified(&root, &path, &digest)?;
        files.insert(path, bytes);
    }
    let model = read_model(&manifest.entry, deps, cache, |name| {
        files
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unlisted payload {name}"))
    })?;
    if model.identity.as_str() != manifest.identity
        || model.version != manifest.version
        || model.rule_set_version != manifest.rule_set_version
    {
        return Err("HTTP source disagrees with manifest".into());
    }
    if let Some(file) = files
        .keys()
        .find(|p| p.ends_with(".ttl") && !model.source_files.contains_key(*p))
    {
        return Err(format!("unreachable ontology payload {file}"));
    }
    Ok(model)
}

mod bundle;
mod corpus;
mod obligations;
mod schema;
mod wire;
pub use bundle::*;
pub use corpus::{Case, cases};
pub use obligations::*;

pub use wire::compare_observations;
pub mod overlay;
pub mod rust;
pub use overlay::{TargetOverlay, load_overlay};
pub use rust::emit_rust;
mod typescript;
pub use typescript::emit_typescript;

mod evolution;
pub use evolution::*;

mod shapes;
pub use shapes::{emit_codec_rust, emit_codec_typescript};
