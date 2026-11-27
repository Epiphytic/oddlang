//! Rebuild the Phase 7 workspace or replay the archived qualification offline.
#[path = "phase7/coverage.rs"]
mod coverage;
#[path = "phase7/frame.rs"]
mod frame;
#[path = "phase7/reconstruction.rs"]
mod reconstruction;

use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use flate2::read::GzDecoder;
use marmish_inventory::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread,
};
fn check_platforms(platforms: &Value) -> Result<()> {
    ensure!(
        *platforms
            == json!({"aarch64-apple-darwin":"observed","x86_64-unknown-linux-gnu":"unobserved","aarch64-unknown-linux-gnu":"unobserved"}),
        "platform observation missing or changed"
    );
    Ok(())
}
fn emit(path: &Path, value: &Value, replay: bool) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if replay {
        ensure!(
            read(path)? == bytes,
            "stale qualification aggregate: {}",
            path.display()
        );
    } else {
        fs::write(path, bytes)?;
    }
    Ok(())
}
fn summarize(root: &Path, dir: &Path, replay: bool) -> Result<Value> {
    let releases = root.join("examples/marmish");
    let inventory_path = dir.join("native/original/inventory/inventory.json.gz");
    let raw = read(&inventory_path)?;
    ensure!(
        read_json(&releases.join("inventory-summary.json"))?["inventorySha256"] == digest(&raw),
        "original source inventory differs from published baseline"
    );
    let inventory = serde_json::from_reader(GzDecoder::new(raw.as_slice()))?;
    let reconstruction = reconstruction::build_pair(&dir.join("rebuild"), &inventory, &releases)?;
    let native = read_json(&dir.join("native/qualification.json"))?;
    check_platforms(&native["platforms"])?;
    let observed = candidate::compare(
        &dir.join("native"),
        PIN,
        text(&reconstruction["candidateCommit"])?,
        text(&reconstruction["candidateTree"])?,
        text(&native["verifierSha256"])?,
    )?;
    ensure!(
        native == observed,
        "native summary disagrees with raw native observations"
    );
    let wire = read_json(&dir.join("wire.json"))?;
    let contract = read_json(&releases.join("observations.json"))?;
    frame::check_wire(&contract, &wire)?;
    frame::check_frame(root, dir, &contract)?;
    let generated = keys(&reconstruction["generated"])?;
    let register = run_json(
        Command::new("cargo")
            .args([
                "+1.97.1",
                "run",
                "--quiet",
                "--locked",
                "--offline",
                "--manifest-path",
            ])
            .arg(root.join("verification/marmish-inventory/Cargo.toml"))
            .args(["--bin", "marmish-gaps", "--"])
            .arg(inventory_path)
            .args(["-", "--generated"])
            .args(&generated),
    )?;
    ensure!(
        register == read_json(&releases.join("gaps.json"))?,
        "published residual register differs from the actual producer coverage"
    );
    let coverage = coverage::coverage(&inventory, &generated, &register)?;
    emit(&dir.join("coverage.json"), &coverage, replay)?;
    let lexical: Map = ["knownSites", "retainedSites", "regeneratedSites"]
        .into_iter()
        .map(|k| (k.into(), coverage[k].clone()))
        .collect();
    let exchanges = array(&wire["directions"])?
        .iter()
        .map(|r| array(&r["exchanges"]).map(Vec::len))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();
    let summary = json!({
        "scope":"native Apple Silicon source-build comparison and framing component conformance",
        "originalCommit":native["originalCommit"],"candidateCommit":native["candidateCommit"],"candidateTree":reconstruction["candidateTree"],
        "independentBuilds":2,"sourceFiles":reconstruction["sourceFiles"],"retainedFilesBeforeIndexUpdate":reconstruction["retainedFiles"],
        "regeneratedRust":generated,"regeneratedIndex":reconstruction["regeneratedOntology"],
        "lexicalSites":lexical,"compiledRuntimeDenominator":null,"platforms":native["platforms"],
        "nativeVerdict":native["status"],"newRegressions":native["regressions"],"frameCases":array(&contract["cases"])?.len(),
        "mixedWireExchanges":exchanges,"inputs":file_hashes(dir,&["coverage.json","wire.json","frame/observations/report.json","native/qualification.json","rebuild/rebuild.json"])?,
        "wholeApplicationRegenerated":false,"universalBehaviorEquivalence":"open"
    });
    emit(&dir.join("qualification.json"), &summary, replay)?;
    Ok(summary)
}
fn passed(status: i32, result: &Value) -> Result<()> {
    ensure!(
        status == 0
            && array(&result["newRegressions"])?.is_empty()
            && matches!(
                result["nativeVerdict"].as_str(),
                Some("passed measured observations" | "baseline failures remain")
            ),
        "native candidate regression; inspect qualification.json"
    );
    Ok(())
}
fn run(root: &Path, mut command: Command) -> Result<ExitStatus> {
    command
        .current_dir(root)
        .status()
        .with_context(|| format!("start {command:?}"))
}
fn verify(root: &Path, upstream: &Path, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    ensure!(
        fs::read_dir(dir)?.next().is_none(),
        "verify requires an empty destination"
    );
    let dir = dir.canonicalize()?;
    let candidate = dir.join("rebuild/candidate");
    let mut rebuild = Command::new("python3");
    rebuild
        .args(["-B", "scripts/marmish/rebuild.py"])
        .arg(upstream)
        .arg(dir.join("rebuild"));
    ensure!(run(root, rebuild)?.success(), "failed reconstruction");
    let mut native = Command::new("cargo");
    native
        .args([
            "+1.97.1",
            "run",
            "--quiet",
            "--locked",
            "--offline",
            "--manifest-path",
            "verification/marmish-inventory/Cargo.toml",
            "--bin",
            "marmish-candidate",
            "--",
        ])
        .arg(upstream)
        .arg(&candidate)
        .arg(dir.join("native"));
    let mut wire = Command::new(root.join("scripts/marmish/offline.sh"));
    wire.args([
        "cargo",
        "+1.97.1",
        "run",
        "--quiet",
        "--offline",
        "--locked",
        "--manifest-path",
        "verification/marmish-inventory/Cargo.toml",
        "--bin",
        "framing-observer",
        "--",
        "compare",
    ])
    .arg(upstream)
    .arg(&candidate)
    .arg(dir.join("wire.json"));
    let mut frame = Command::new(root.join("scripts/qualify-marmish.sh"));
    frame.arg("frame").arg(upstream).arg(dir.join("frame"));
    let statuses = thread::scope(|s| {
        let children = [native, wire, frame].map(|cmd| s.spawn(move || run(root, cmd)));
        children
            .into_iter()
            .map(|job| {
                job.join()
                    .map_err(|_| anyhow::anyhow!("qualification worker panicked"))?
            })
            .collect::<Result<Vec<_>>>()
    })?;
    let code = statuses[0].code().unwrap_or(-1);
    ensure!(
        matches!(code, 0 | 3) && statuses[1..].iter().all(ExitStatus::success),
        "incomplete qualification commands: {statuses:?}"
    );
    let result = summarize(root, &dir, false)?;
    passed(code, &result)?;
    println!(
        "Qualified {} in {}",
        text(&result["candidateCommit"])?,
        dir.display()
    );
    Ok(())
}
#[derive(Parser)]
struct Args {
    /// Pinned source checkout for a fresh run or 'replay' for an archived run.
    upstream_or_replay: PathBuf,
    output: PathBuf,
}
fn main() -> Result<()> {
    let args = Args::parse();
    if args.upstream_or_replay == Path::new("replay") {
        let result = summarize(&root(), &args.output.canonicalize()?, true)?;
        passed(0, &result)?;
        println!(
            "Verified archived qualification for {}: {}",
            text(&result["candidateCommit"])?,
            text(&result["nativeVerdict"])?
        );
    } else {
        verify(
            &root(),
            &args.upstream_or_replay.canonicalize()?,
            &args.output,
        )?;
    }
    Ok(())
}
#[cfg(test)]
#[path = "phase7/tests.rs"]
mod tests;
