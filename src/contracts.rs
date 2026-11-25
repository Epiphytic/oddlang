//! Offline resolution of immutable ledger contract releases.
mod payloads;
use crate::model::{Model, core, model_from_graph};
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{Graph, Term, TripleRef, vocab::xsd},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pin {
    pub identity: String,
    pub version: String,
    pub manifest_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    identity: String,
    version: String,
    entry: String,
    files: BTreeMap<String, String>,
}

pub struct Release {
    pub version: String,
    pub graph: Graph,
    pub model: Model,
    files: BTreeMap<String, Vec<u8>>,
}

impl Release {
    pub fn payload(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(Vec::as_slice)
    }
}

pub(crate) fn normal(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && name.split('/').all(|part| !matches!(part, "" | "." | ".."))
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(crate) fn contained(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !normal(name) {
        return Err(format!("invalid cache path {name}"));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("cache root: {e}"))?;
    let path = root
        .join(name)
        .canonicalize()
        .map_err(|e| format!("missing or unreadable cached {name}: {e}"))?;
    if !path.starts_with(&root) {
        return Err(format!("cache path escapes root: {name}"));
    }
    Ok(path)
}

pub(crate) fn verified(root: &Path, name: &str, digest: &str) -> Result<Vec<u8>, String> {
    let bytes = fs::read(contained(root, name)?).map_err(|e| format!("{name}: {e}"))?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != digest {
        return Err(format!("digest mismatch for {name}"));
    }
    Ok(bytes)
}

/// Load the release selected by a project's import IRI and its workspace lock.
/// The project is a Turtle file. The cache is the directory containing ledger/v1, ledger/v2, etc.
pub fn resolve(project: &Path, lock: &Path, cache: &Path) -> Result<Release, String> {
    let pin: Pin =
        serde_json::from_slice(&fs::read(lock).map_err(|e| format!("workspace lock: {e}"))?)
            .map_err(|e| format!("workspace lock: {e}"))?;
    let expected = format!("{}:{}", pin.identity, pin.version);
    let project_bytes = fs::read(project).map_err(|e| format!("project ontology: {e}"))?;
    let mut imports = BTreeSet::new();
    for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(project_bytes.as_slice()) {
        let quad = quad.map_err(|e| format!("project ontology: {e}"))?;
        if quad.predicate == core("imports") {
            let Term::NamedNode(iri) = quad.object else {
                return Err("expected versioned contract IRI".into());
            };
            imports.insert(iri.into_string());
        }
    }
    if imports.len() != 1 || !(imports.contains(&expected) || imports.contains(&pin.identity)) {
        return Err(format!(
            "project imports {imports:?}, workspace pins {expected}"
        ));
    }
    resolve_pin(&pin, cache)
}

pub fn resolve_pin(pin: &Pin, cache: &Path) -> Result<Release, String> {
    if pin.identity != "urn:oddlang:contract:ledger"
        || pin.version.is_empty()
        || !pin
            .version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err("unsupported release identity or invalid version".into());
    }
    let root = contained(cache, &format!("ledger/{}", pin.version))?;
    let bytes = verified(&root, "manifest.json", &pin.manifest_sha256)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("manifest.json: {e}"))?;
    if manifest.identity != pin.identity || manifest.version != pin.version {
        return Err("manifest identity or version disagrees with workspace lock".into());
    }
    let mut files = BTreeMap::new();
    for (name, digest) in manifest.files {
        let bytes = verified(&root, &name, &digest)?;
        files.insert(name, bytes);
    }
    for required in ["wire.json", "vectors.json", "compatibility.md"] {
        if !files.contains_key(required) {
            return Err(format!("unlisted payload {required}"));
        }
    }
    if !normal(&manifest.entry) {
        return Err(format!("invalid entry {}", manifest.entry));
    }
    let mut graph = Graph::new();
    let mut loaded = BTreeSet::new();
    let mut pending = BTreeSet::from([PathBuf::from(manifest.entry)]);
    while let Some(path) = pending.pop_first() {
        if !loaded.insert(path.clone()) {
            continue;
        }
        let bytes = path
            .to_str()
            .and_then(|name| files.get(name))
            .ok_or_else(|| format!("unlisted ontology import {}", path.display()))?;
        for quad in RdfParser::from_format(RdfFormat::Turtle)
            .rename_blank_nodes()
            .for_reader(bytes.as_slice())
        {
            let quad = quad.map_err(|e| format!("{}: {e}", path.display()))?;
            if quad.predicate == core("imports") {
                let Term::Literal(value) = &quad.object else {
                    return Err("expected relative ontology import".into());
                };
                if value.datatype() != xsd::STRING || !normal(value.value()) {
                    return Err(format!(
                        "invalid relative ontology import {}",
                        value.value()
                    ));
                }
                pending.insert(path.parent().unwrap_or(Path::new("")).join(value.value()));
            }
            graph.insert(TripleRef::new(&quad.subject, &quad.predicate, &quad.object));
        }
    }
    for name in files
        .keys()
        .filter(|p| p.starts_with("ontology/") || p.ends_with(".ttl"))
    {
        if !loaded.contains(Path::new(name)) {
            return Err(format!("unreachable ontology payload {name}"));
        }
    }
    let model = model_from_graph(graph.clone(), loaded)?;
    payloads::validate(
        &pin.version,
        &model,
        &files["wire.json"],
        &files["vectors.json"],
    )?;
    Ok(Release {
        version: pin.version.clone(),
        graph,
        model,
        files,
    })
}
