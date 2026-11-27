//! Reauthorize one graph leaf and run the independent local wire oracle.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use marmish_inventory::{
    Set, digest, object, read, read_json,
    stream_workspace::{Tools, closure, denied_reads, executable, git_archive, profile, save},
    text,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
const MODULE: &str = "src/copy_protocol.rs";
const LEAF: &str =
    "<urn:oddlang:rust-tree:file:src/copy_protocol.rs#7816> <urn:oddlang:rust-tree:spelling>";
#[derive(Parser)]
struct Args {
    #[arg(long)]
    repository: PathBuf,
    #[arg(long)]
    oddlang: PathBuf,
    #[arg(long)]
    candidate: PathBuf,
    #[arg(long)]
    baseline: PathBuf,
    #[arg(long)]
    vendor: PathBuf,
    #[arg(long)]
    vendor_config: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    commit: String,
}
fn replace_once(raw: &str, before: &str, after: &str) -> Result<String> {
    ensure!(
        raw.matches(before).count() == 1,
        "expected one occurrence of {before}"
    );
    Ok(raw.replacen(before, after, 1))
}
fn rewrite(bundle: &Path, source: &Path) -> Result<String> {
    let graph = bundle.join("syntax.ttl");
    let inventory = bundle.join("sources.json");
    let lock = bundle.join("bundle.lock");
    let graph_text = fs::read_to_string(&graph)?;
    let changed = replace_once(
        &graph_text,
        &format!("{LEAF} \"5\" .\n"),
        &format!("{LEAF} \"12\" .\n"),
    )?;
    let src = fs::read_to_string(source)?;
    let inv_text = fs::read_to_string(&inventory)?;
    let inv: Value = serde_json::from_str(&inv_text)?;
    let original = text(&inv["files"][MODULE])?;
    ensure!(
        digest(src.as_bytes()) == original,
        "expected the verified emitted copy protocol source"
    );
    let mutated = replace_once(
        &src,
        "payload.extend(bytes);\n                5\n",
        "payload.extend(bytes);\n                12\n",
    )?;
    let changed_inventory = replace_once(&inv_text, original, &digest(mutated.as_bytes()))?;
    let lock_text = fs::read_to_string(&lock)?;
    let locked: Value = serde_json::from_str(&lock_text)?;
    let graph_digest = text(&locked["syntaxSha256"])?;
    let inv_digest = text(&locked["sourcesSha256"])?;
    ensure!(
        graph_digest == digest(graph_text.as_bytes()) && inv_digest == digest(inv_text.as_bytes()),
        "base release input digests changed"
    );
    let updated = replace_once(
        &replace_once(&lock_text, graph_digest, &digest(changed.as_bytes()))?,
        inv_digest,
        &digest(changed_inventory.as_bytes()),
    )?;
    fs::write(graph, changed)?;
    fs::write(inventory, changed_inventory)?;
    fs::write(lock, &updated)?;
    Ok(digest(updated.as_bytes()))
}
fn has_rust(dir: &Path) -> Result<bool> {
    for entry in fs::read_dir(dir)? {
        let e = entry?;
        if e.file_type()?.is_dir() {
            if has_rust(&e.path())? {
                return Ok(true);
            }
        } else if e.path().extension().is_some_and(|s| s == "rs") {
            return Ok(true);
        }
    }
    Ok(false)
}
fn main() -> Result<()> {
    let args = Args::parse();
    let tools = Tools::discover()?;
    let helper = executable("provider-validation", "build-closure")?;
    let driver = executable("marmish-inventory", "stream-driver")?;
    fs::create_dir(&args.output)?;
    let out = args.output.canonicalize()?;
    let root = out.join("mutated");
    let archive = git_archive(&args.repository, &args.commit, &root)?;
    let pkg = root.join("crates/marmish-stream");
    ensure!(!has_rust(&pkg)?, "expected a source-free Git archive");
    let bundle = root.join("oddlang/stream");
    let base = digest(&read(&bundle.join("bundle.lock"))?);
    let release = root.join("oddlang/stream.sha256");
    ensure!(
        fs::read_to_string(&release)?.trim() == base,
        "base bundle differs from committed release pin"
    );
    let original = args
        .candidate
        .canonicalize()?
        .join("crates/marmish-stream")
        .join(MODULE);
    let pin = rewrite(&bundle, &original)?;
    fs::write(&release, format!("{pin}\n"))?;
    let (mut trees, files) = tools.boundaries();
    let writable = out.join("writable");
    let env = tools.environment(&writable, &root)?;
    let emit = out.join("emit.sb");
    let oddlang = args.oddlang.canonicalize()?;
    let mut emit_files = files.clone();
    emit_files.push(oddlang.clone());
    for e in fs::read_dir(&bundle)? {
        emit_files.push(e?.path())
    }
    fs::write(&emit, profile(&helper, &trees, &writable, &emit_files)?)?;
    let mut probes = denied_reads(
        &emit,
        json!({"unmodifiedCandidate":original}),
        &writable,
        &env,
    )?;
    let emitted = writable.join("generated");
    closure(
        &helper,
        json!({"op":"run","command":[oddlang,"build","rust-tree",bundle,emitted,"--bundle-sha256",pin],"name":"emit","dir":writable,"env":env,"out":out,"sandbox":emit,"success":true}),
    )?;
    fs::rename(emitted.join("src"), pkg.join("src"))?;
    fs::copy(&args.vendor_config, writable.join("cargo/config.toml"))?;
    let build = out.join("build.sb");
    trees.extend([root.clone(), args.vendor.canonicalize()?]);
    fs::write(&build, profile(&helper, &trees, &writable, &files)?)?;
    probes.as_object_mut().unwrap().extend(
        object(&denied_reads(
            &build,
            json!({"unmodifiedCandidateDuringBuild":original}),
            &root,
            &env,
        )?)?
        .clone(),
    );
    let receipt = out.join("oracle-mutation.json");
    let status = Command::new(driver)
        .arg("--workspace")
        .arg(&root)
        .arg("--target-dir")
        .arg(writable.join("oracle-target"))
        .arg("--receipt")
        .arg(&receipt)
        .args(["--role", "mutation", "--baseline"])
        .arg(args.baseline.canonicalize()?)
        .args([
            "--commit",
            &args.commit,
            "--archive-sha256",
            &archive,
            "--toolchain",
        ])
        .arg(tools.chain)
        .arg("--sandbox")
        .arg(build)
        .current_dir(&root)
        .env_clear()
        .envs(env)
        .status()?;
    ensure!(
        !status.success() && receipt.is_file(),
        "mutation did not build and reach the independent oracle"
    );
    let obs = read_json(&receipt)?;
    let mut mismatches = Set::new();
    for row in marmish_inventory::array(&obs["observations"])? {
        if !row["matched"].as_bool().context("matched")? {
            mismatches.insert(text(&row["name"])?.to_owned());
        }
    }
    ensure!(
        mismatches == marmish_inventory::set(&["binary-data-encode", "maximum-data-encode"]),
        "unexpected mutation case spread: {mismatches:?}"
    );
    save(
        &out.join("report.json"),
        &json!({"baseBundleSha256":base,"bundleSha256":pin,"oracleSha256":digest(&read(&receipt)?),"changedLeaf":LEAF,"deniedReads":probes,"observedDifferences":mismatches}),
    )
}
