//! Seal or replay independently observed native, CLI, and framing evidence.
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
const REV: &str = "49fc705408a230bd0ad896248d08dbdd2faad05a";
const CONTRACT: &str = include_str!("../../../../examples/marmish/observations.json");
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    Check {
        directory: PathBuf,
        #[arg(long)]
        write: bool,
    },
    Cli {
        baseline: PathBuf,
    },
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_reader(fs::File::open(path)?)?)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn sha(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    io::copy(&mut f, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().context("missing text")
}
fn list(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("missing array")
}
fn observed(data: &[u8]) -> Value {
    json!({"byteLength":data.len(),"sha256":hash(data)})
}
#[path = "../framing_contract.rs"]
mod framing_contract;
use framing_contract::cases;
fn commands(native: &Value) -> Result<BTreeMap<&str, &Value>> {
    let mut result = BTreeMap::new();
    for row in list(&native["commands"])? {
        let name = text(&row["name"])?;
        ensure!(
            !name.is_empty() && Path::new(name).file_name().is_some_and(|v| v == name),
            "invalid native command name"
        );
        ensure!(
            result.insert(name, row).is_none(),
            "missing or duplicate native command"
        );
    }
    Ok(result)
}
fn verify_cli(native: &Value, root: &Path, contract: &Value) -> Result<()> {
    ensure!(
        native["revision"] == REV,
        "CLI observations describe the wrong revision"
    );
    let rows = commands(native)?;
    let release = rows.get("release").copied().unwrap_or(&Value::Null);
    ensure!(
        release["exitCode"] == 0 && release["timedOut"] == false && release["status"] == "passed",
        "release build failed or is missing"
    );
    ensure!(
        release["argv"]
            == json!([
                "cargo",
                "build",
                "--locked",
                "--offline",
                "--release",
                "-p",
                "marmish-native",
                "--bins"
            ]),
        "unexpected release build command"
    );
    let binaries = native["binaries"]
        .as_object()
        .context("missing release binaries")?;
    ensure!(
        binaries.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == BTreeSet::from(["marmish", "marmish-worker", "marmishd"]),
        "incomplete release binary inventory"
    );
    for identity in binaries.values() {
        let digest = text(&identity["sha256"])?;
        ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                && identity["size"].as_u64().unwrap_or(0) > 0,
            "invalid release binary identity"
        );
    }
    let mut probes = BTreeSet::new();
    for case in list(&contract["cases"])? {
        let binary = text(&case["binary"])?;
        let arg = text(&case["argument"])?;
        ensure!(
            binaries.contains_key(binary)
                && matches!(arg, "--help" | "--version")
                && probes.insert((binary, arg)),
            "invalid frozen CLI probe"
        );
        let name = format!("{binary}-{}", arg.trim_start_matches("--"));
        let row = rows
            .get(name.as_str())
            .context("missing or incorrect CLI command")?;
        ensure!(
            row["argv"] == json!([format!("$RUN/target/release/{binary}"), arg]),
            "missing or incorrect CLI command: {name}"
        );
        ensure!(
            row["timedOut"] == false
                && row["exitCode"] == case["exitCode"]
                && row["status"]
                    == if case["exitCode"] == 0 {
                        "passed"
                    } else {
                        "failed"
                    },
            "unexpected CLI exit: {name}"
        );
        for stream in ["stdout", "stderr"] {
            let path = format!("logs/{name}.{stream}");
            ensure!(row[stream] == path, "wrong CLI log");
            let bytes = fs::read(root.join(path))?;
            let identity = observed(&bytes);
            ensure!(
                case[stream] == identity && row[format!("{stream}Sha256")] == identity["sha256"],
                "unexpected CLI bytes: {name} {stream}"
            );
            if arg == "--version" && binary != "marmish-worker" && stream == "stdout" {
                ensure!(
                    bytes == text(&contract["productVersionStdoutUtf8"])?.as_bytes(),
                    "unexpected version text"
                );
            }
        }
    }
    ensure!(
        probes.len() == 6,
        "frozen CLI contract must cover exactly six product probes"
    );
    Ok(())
}
fn native_logs(root: &Path, native: &Value) -> Result<()> {
    ensure!(
        native["revision"] == REV,
        "missing pinned native observations"
    );
    let rows = commands(native)?;
    for name in [
        "ontology",
        "metadata",
        "installed-metadata",
        "workspace",
        "workspace-ignored",
        "installed",
        "installed-ignored",
        "release",
        "cfg",
        "rustc",
        "cargo",
    ] {
        ensure!(
            rows.contains_key(name),
            "missing or duplicate native command"
        );
    }
    for (name, row) in rows {
        for stream in ["stdout", "stderr"] {
            let path = format!("logs/{name}.{stream}");
            ensure!(
                row[stream] == path && row[format!("{stream}Sha256")] == sha(&root.join(path))?,
                "native command log changed"
            );
        }
        ensure!(
            (row["exitCode"] == 0) == (row["status"] == "passed"),
            "native command status disagrees with exit code"
        );
    }
    Ok(())
}
fn collect_files(root: &Path, rel: &Path, out: &mut BTreeMap<PathBuf, String>) -> Result<()> {
    for entry in fs::read_dir(root.join(rel))? {
        let entry = entry?;
        let typ = entry.file_type()?;
        let name = rel.join(entry.file_name());
        ensure!(
            !typ.is_symlink(),
            "unexpected symlink in receipt: {}",
            name.display()
        );
        if typ.is_dir() {
            collect_files(root, &name, out)?;
        } else if typ.is_file() && name != Path::new("receipt.json") {
            out.insert(name, sha(&entry.path())?);
        }
    }
    Ok(())
}
fn check(root: &Path, write: bool) -> Result<()> {
    let mut files = BTreeMap::new();
    collect_files(root, Path::new(""), &mut files)?;
    let native = read(&root.join("native/baseline.json"))?;
    native_logs(&root.join("native"), &native)?;
    let contract: Value = serde_json::from_str(CONTRACT)?;
    verify_cli(&native, &root.join("native"), &contract["cli"])?;
    let inventory = read(&root.join("inventory/inventory-summary.json"))?;
    let observations = read(&root.join("observations/results.json"))?;
    ensure!(
        inventory["gitRevision"] == REV && observations["sourceRevision"] == REV,
        "component evidence describes another revision"
    );
    ensure!(
        observations["contractSha256"] == hash(CONTRACT.as_bytes())
            && observations["cases"] == cases(&contract)?,
        "framing observations differ from the frozen contract"
    );
    ensure!(
        inventory["inventorySha256"] == sha(&root.join("inventory/inventory.json.gz"))?,
        "inventory does not match its summary"
    );
    let receipt = json!({"schemaVersion":1,"sourceRevision":REV,"files":files});
    let path = root.join("receipt.json");
    if write {
        let mut staged = tempfile::NamedTempFile::new_in(root)?;
        serde_json::to_writer_pretty(&mut staged, &receipt)?;
        staged.write_all(b"\n")?;
        staged.persist_noclobber(&path)?;
    }
    ensure!(
        read(&path)? == receipt,
        "baseline evidence changed after collection"
    );
    Ok(())
}
fn main() -> Result<()> {
    match Args::parse().action {
        Action::Check { directory, write } => check(&directory, write),
        Action::Cli { baseline } => verify_cli(
            &read(&baseline)?,
            baseline.parent().context("missing parent")?,
            &serde_json::from_str::<Value>(CONTRACT)?["cli"],
        ),
    }
}

#[cfg(test)]
#[path = "receipt/tests.rs"]
mod tests;
