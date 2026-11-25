//! Isolated, reproducible import of a selected Rust source profile.
pub mod cleanup;
mod lower;

use crate::model::{BALANCE, CORE, load_model};
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{Graph, Term, vocab::rdf},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

type Res<T> = Result<T, String>;
type Digests = BTreeMap<String, String>;
const OVERLAYS: [&str; 4] = ["avro.ttl", "fields.ttl", "rust.ttl", "sql.ttl"];
const INPUTS: [&str; 3] = ["Cargo.toml", "Cargo.lock", "src/lib.rs"];
const ROOT: &str = "@prefix o: <urn:oddlang:core:> .\n<urn:oddlang:ledger:Domain> o:imports \"fields.ttl\", \"rust.ttl\", \"sql.ttl\", \"avro.ttl\", \"operations.ttl\" .\n";
const COMPILER: &str = "31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    profile: String,
    mapping: lower::Mapping,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportLock {
    version: u32,
    profile: String,
    compiler: String,
    target: String,
    flags: Vec<String>,
    inputs: Digests,
    mapping_sha256: String,
    overlays: Digests,
    files: Digests,
    graph_sha256: String,
    cleanup: Option<serde_json::Value>,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn read(path: &Path) -> Res<Vec<u8>> {
    let md = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !md.is_file() {
        return Err(format!("expected regular file: {}", path.display()));
    }
    fs::read(path).map_err(|e| e.to_string())
}
fn put(dir: &Path, name: &str, data: impl AsRef<[u8]>) -> Res<()> {
    fs::write(dir.join(name), data).map_err(|e| format!("{name}: {e}"))
}
fn json(v: &impl Serialize) -> Res<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn as_utf8(bytes: &[u8]) -> Res<&str> {
    std::str::from_utf8(bytes).map_err(|e| e.to_string())
}
fn run(cmd: &mut Command) -> Res<String> {
    let output = cmd
        .output()
        .map_err(|e| format!("{}: {e}", cmd.get_program().to_string_lossy()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
fn check_manifest(bytes: &[u8]) -> Res<()> {
    let doc: toml::Table =
        toml::from_str(as_utf8(bytes)?).map_err(|e| format!("Cargo.toml: {e}"))?;
    for (name, value) in &doc {
        match name.as_str() {
            "package" => {
                let table = value.as_table().ok_or("invalid Cargo package")?;
                if table.get("edition").and_then(|v| v.as_str()) != Some("2024")
                    || table
                        .keys()
                        .any(|k| !matches!(k.as_str(), "name" | "version" | "edition" | "publish"))
                {
                    return Err("unsupported ledger-v1 package settings or edition".into());
                }
            }
            "workspace" | "dependencies" if value.as_table().is_some_and(|t| t.is_empty()) => {}
            _ => return Err(format!("unsupported Cargo declaration {name}")),
        }
    }
    if !doc.contains_key("package") {
        return Err("missing Cargo package".into());
    }
    Ok(())
}
fn compile(source: &BTreeMap<String, Vec<u8>>) -> Res<(String, String)> {
    check_manifest(&source["Cargo.toml"])?;
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let root = scratch.path();
    fs::create_dir(root.join("src")).map_err(|e| e.to_string())?;
    for (name, bytes) in source {
        put(root, name, bytes)?;
    }
    let rustc = run(Command::new("rustup").args(["which", "rustc"]))?;
    let cargo = run(Command::new("rustup").args(["which", "cargo"]))?;
    let compiler = run(Command::new(rustc.trim()).arg("-vV"))?;
    if !compiler
        .lines()
        .any(|l| l == format!("commit-hash: {COMPILER}"))
    {
        return Err("ledger-v1 requires rustc 1.96.1 (31fca3adb)".into());
    }
    let target = compiler
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("missing compiler host")?
        .to_owned();
    // Run the pinned compiler and Cargo outside the input tree. Local build scripts,
    // ambient rustflags and user Cargo configuration cannot extend the selected profile.
    let mut cmd = Command::new(cargo.trim());
    cmd.current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("CARGO_HOME", root.join("cargo-home"))
        .env("RUSTC", rustc.trim())
        .args([
            "rustc",
            "--lib",
            "--locked",
            "--offline",
            "--target",
            &target,
            "--",
            "-C",
            "overflow-checks=yes",
        ]);
    run(&mut cmd).map_err(|e| {
        format!(
            "Rust build failed: {}",
            e.replace(&root.display().to_string(), "<input>")
        )
    })?;
    Ok((compiler, target))
}

fn overlay(bytes: &[u8]) -> Res<String> {
    let mut g = Graph::new();
    for q in RdfParser::from_format(RdfFormat::Turtle).for_reader(bytes) {
        let t = q.map_err(|e| e.to_string())?;
        if t.predicate == rdf::TYPE {
            let Term::NamedNode(n) = &t.object else {
                return Err("invalid overlay class".into());
            };
            if !matches!(
                n.as_str().strip_prefix(CORE),
                Some("Entity" | "Field" | "Projection")
            ) {
                return Err("executable declaration in structural overlay".into());
            }
        } else {
            match t.predicate.as_str().strip_prefix(CORE) {
                Some("imports") if matches!(&t.object,Term::Literal(v) if v.value()=="fields.ttl") =>
                    {}
                Some(
                    "type" | "field" | "key" | "range" | "min" | "max" | "name" | "mapping"
                    | "target" | "projects" | "alias" | "dependsOn",
                ) => {}
                _ => {
                    return Err(format!(
                        "unexpected structural overlay predicate {}",
                        t.predicate
                    ));
                }
            }
        }
        g.insert(oxigraph::model::TripleRef::from(t.as_ref()));
    }
    g.canonicalize(oxigraph::model::graph::CanonicalizationAlgorithm::Unstable);
    let rows: BTreeSet<_> = g.iter().map(|t| format!("{t} .\n")).collect();
    Ok(rows.into_iter().collect())
}
fn graph_digest(files: &Digests) -> Res<String> {
    let semantic: BTreeMap<_, _> = files.iter().filter(|(k, _)| k.ends_with(".ttl")).collect();
    Ok(digest(&json(&semantic)?))
}
fn project_files(dir: &Path) -> Res<Digests> {
    let mut files = Digests::new();
    for name in OVERLAYS
        .into_iter()
        .chain(["domain.ttl", "operations.ttl", "source-map.json"])
    {
        files.insert(name.into(), digest(&read(&dir.join(name))?));
    }
    Ok(files)
}
fn validate_root(root: &Path) -> Res<()> {
    for extra in [
        "build.rs",
        ".cargo/config",
        ".cargo/config.toml",
        "rust-toolchain",
        "rust-toolchain.toml",
    ] {
        if root.join(extra).symlink_metadata().is_ok() {
            return Err(format!("unsupported ledger-v1 input {extra}"));
        }
    }
    if fs::symlink_metadata(root.join("src"))
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("symlinked Rust source directory".into());
    }
    for entry in fs::read_dir(root.join("src")).map_err(|e| e.to_string())? {
        if entry.map_err(|e| e.to_string())?.file_name() != "lib.rs" {
            return Err("ledger-v1 requires a single source module".into());
        }
    }
    Ok(())
}
pub fn import(manifest: &Path, map: &Path, out: &Path, receipt: Option<&Path>) -> Res<()> {
    if manifest.file_name().is_none_or(|n| n != "Cargo.toml") {
        return Err("expected Cargo.toml".into());
    }
    if out.exists()
        && fs::read_dir(out)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("import needs an empty output directory".into());
    }
    let root = manifest.parent().unwrap_or(Path::new("."));
    validate_root(root)?;
    let input: BTreeMap<_, _> = INPUTS
        .into_iter()
        .map(|name| Ok((name.to_owned(), read(&root.join(name))?)))
        .collect::<Res<_>>()?;
    let map_bytes = read(map)?;
    let config: Profile =
        toml::from_str(as_utf8(&map_bytes)?).map_err(|e| format!("mapping.toml: {e}"))?;
    if config.profile != "ledger-v1" {
        return Err(format!("unsupported import profile {}", config.profile));
    }
    let cleanup = receipt
        .map(|p| cleanup::verify(p, manifest, map))
        .transpose()?;
    let (operations, spans) = lower::lower(as_utf8(&input["src/lib.rs"])?, &config.mapping)?;
    let (compiler, target) = compile(&input)?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let dir = temp.path();
    let overlay_dir = map.parent().unwrap_or(Path::new(".")).join("overlays");
    if overlay_dir
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("symlinked overlays directory".into());
    }
    let mut overlays = Digests::new();
    for name in OVERLAYS {
        let bytes = read(&overlay_dir.join(name))?;
        overlays.insert(name.into(), digest(&bytes));
        put(dir, name, overlay(&bytes)?)?;
    }
    put(dir, "domain.ttl", ROOT)?;
    put(dir, "operations.ttl", operations)?;
    put(dir, "source-map.json", json(&spans)?)?;
    let model = load_model(dir)?;
    if model.fields.get(BALANCE).and_then(|f| f.range)
        != Some((config.mapping.min, config.mapping.max))
        || model.fields.len() != 3
        || model.operations.len() != 1
    {
        return Err("source and structural overlay declarations or balance bounds disagree".into());
    }
    let inputs = input.iter().map(|(k, v)| (k.clone(), digest(v))).collect();
    let files = project_files(dir)?;
    let lock = ImportLock {
        version: 1,
        profile: config.profile,
        compiler,
        target,
        flags: vec!["-C".into(), "overflow-checks=yes".into()],
        inputs,
        mapping_sha256: digest(&map_bytes),
        overlays,
        graph_sha256: graph_digest(&files)?,
        files,
        cleanup,
    };
    put(dir, "import.lock", json(&lock)?)?;
    fs::rename(dir, out).map_err(|e| format!("publish imported project: {e}"))
}

pub fn verify(dir: &Path) -> Res<()> {
    let lock: ImportLock =
        serde_json::from_slice(&read(&dir.join("import.lock"))?).map_err(|e| e.to_string())?;
    let files = project_files(dir)?;
    if lock.version != 1
        || lock.profile != "ledger-v1"
        || lock.files != files
        || lock.graph_sha256 != graph_digest(&files)?
    {
        return Err("stale or invalid Rust import lock".into());
    }
    load_model(dir).map(|_| ())
}

pub fn replay(manifest: &Path, map: &Path, dir: &Path, receipt: Option<&Path>) -> Res<()> {
    verify(dir)?;
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let copy = temp.path().join("import");
    import(manifest, map, &copy, receipt)?;
    if read(&dir.join("import.lock"))? != read(&copy.join("import.lock"))? {
        return Err("Rust inputs, toolchain or imported artifacts differ from lock".into());
    }
    Ok(())
}

/// Rebuild all three native projections and compare the delivered files byte for byte.
pub fn verify_build(dir: &Path, output: &Path) -> Res<()> {
    verify(dir)?;
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    crate::targets::build(&load_model(dir)?, scratch.path()).map_err(|e| e.to_string())?;
    for name in ["account.rs", "account.avsc", "schema.sql"] {
        if read(&output.join(name))? != read(&scratch.path().join(name))? {
            return Err(format!("stale generated artifact {name}"));
        }
    }
    Ok(())
}
