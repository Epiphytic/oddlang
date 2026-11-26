//! Pinned, lossless native Rust syntax bundles.
mod graph;
mod import;
mod parser;

use crate::contracts::{normal, verified};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
struct FileSyntax {
    bom: bool,
    crlf: Vec<usize>,
    nodes: Vec<Syntax>,
}

#[derive(Debug, PartialEq, Eq)]
enum Syntax {
    Branch {
        kind: String,
        children: Vec<usize>,
    },
    Leaf {
        kind: String,
        lexical_kind: String,
        spelling: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    version: u32,
    package: String,
    edition: String,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lock {
    version: u32,
    syntax_sha256: String,
    sources_sha256: String,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn json(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn publish(output: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err(format!("output already exists: {}", output.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => (),
        Err(error) => return Err(format!("{}: {error}", output.display())),
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staged = tempfile::tempdir_in(parent).map_err(|error| format!("stage output: {error}"))?;
    write(staged.path())?;
    fs::rename(staged.path(), output)
        .map_err(|error| format!("publish {}: {error}", output.display()))
}

fn write_file(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(name);
    fs::create_dir_all(path.parent().expect("output file within stage"))
        .and_then(|()| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
                .write_all(bytes)
        })
        .map_err(|error| format!("write {name}: {error}"))
}

pub fn capture(manifest: &Path, package: &str, output: &Path) -> Result<()> {
    let (edition, files) = import::capture(manifest, package)?;
    let mut sources = Sources {
        version: VERSION,
        package: package.into(),
        edition,
        files: BTreeMap::new(),
    };
    let mut syntax = BTreeMap::new();
    for (path, (bytes, tree)) in files {
        sources.files.insert(path.clone(), digest(&bytes));
        syntax.insert(path, tree);
    }
    let graph = graph::encode(&sources, &syntax);
    let inventory = json(&sources)?;
    let lock = json(&Lock {
        version: VERSION,
        syntax_sha256: digest(&graph),
        sources_sha256: digest(&inventory),
    })?;
    publish(output, |stage| {
        write_file(stage, "syntax.ttl", &graph)?;
        write_file(stage, "sources.json", &inventory)?;
        write_file(stage, "bundle.lock", &lock)
    })
}

pub fn build(bundle: &Path, output: &Path, pin: &str) -> Result<()> {
    let lock: Lock = serde_json::from_slice(&verified(bundle, "bundle.lock", pin)?)
        .map_err(|error| format!("bundle.lock: {error}"))?;
    let inventory = verified(bundle, "sources.json", &lock.sources_sha256)?;
    let sources: Sources =
        serde_json::from_slice(&inventory).map_err(|error| format!("sources.json: {error}"))?;
    if json(&sources)? != inventory {
        return Err("sources.json must use canonical inventory JSON".into());
    }
    if lock.version != VERSION || sources.version != VERSION {
        return Err("unsupported Rust syntax bundle version".into());
    }
    if sources.files.is_empty()
        || sources.package.is_empty()
        || sources
            .files
            .keys()
            .any(|path| !normal(path) || !path.ends_with(".rs"))
    {
        return Err("invalid Rust source inventory path or package".into());
    }
    let graph_path = crate::contracts::contained(bundle, "syntax.ttl")?;
    let bytes = fs::read(graph_path).map_err(|error| format!("syntax.ttl: {error}"))?;
    let graph = graph::Graph::read(&bytes)?;
    if digest(&graph.canonical()) != lock.syntax_sha256 {
        return Err("digest mismatch for syntax.ttl".into());
    }
    publish(output, |stage| {
        let mut reparsed = BTreeMap::new();
        for (path, expected) in &sources.files {
            let text = graph
                .emit(path, &sources.edition)
                .map_err(|error| format!("{path}: {error}"))?;
            if digest(text.as_bytes()) != *expected {
                return Err(format!("generated source digest mismatch for {path}"));
            }
            let parsed = parser::parse(&text, &sources.edition)
                .map_err(|error| format!("{path}: {error}"))?;
            reparsed.insert(path.clone(), parsed);
            write_file(stage, path, text.as_bytes())?;
        }
        if graph::encode(&sources, &reparsed) != graph.canonical() {
            return Err("syntax graph differs from canonical reparse of generated Rust".into());
        }
        Ok(())
    })
}
