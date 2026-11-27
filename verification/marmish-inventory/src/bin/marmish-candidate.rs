//! Compare the native candidate or authenticate and replay an archived comparison.
use anyhow::{Result, ensure};
use clap::Parser;
use marmish_inventory::{PIN, candidate, read_json, text};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Parser)]
struct Compare {
    original: PathBuf,
    candidate: PathBuf,
    output: PathBuf,
    #[arg(long)]
    compare_existing: bool,
}
fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, dir] if mode == "original" => {
            print_json(&candidate::original::recover(Path::new(dir))?)
        }
        [mode, dir, revision, tree] if mode == "replay" => {
            let dir = Path::new(dir);
            let evidence = read_json(&dir.join("qualification.json"))?;
            print_json(&candidate::compare(
                dir,
                PIN,
                revision,
                tree,
                text(&evidence["verifierSha256"])?,
            )?)
        }
        [mode, tmp, report, revision, divider, policy @ ..]
            if mode == "installed-status" && divider == "--" =>
        {
            print_json(&candidate::installed::observe(
                Path::new(tmp),
                Path::new(report),
                revision,
                policy,
                &std::env::vars().collect(),
            )?)
        }
        _ => {
            let args = Compare::parse();
            let upstream = args.original.canonicalize()?;
            let checkout = args.candidate.canonicalize()?;
            let (revision, tree) = candidate::collect::identities(&upstream, &checkout)?;
            if !args.compare_existing {
                candidate::collect::collect(
                    &checkout,
                    &std::path::absolute(&args.output)?,
                    &revision,
                )?;
            }
            let result =
                candidate::compare(&args.output, PIN, &revision, &tree, &candidate::verifier())?;
            let mut json = serde_json::to_vec_pretty(&result)?;
            json.push(b'\n');
            let dest = args.output.join("qualification.json");
            fs::write(&dest, json)?;
            println!("{}: {}", text(&result["status"])?, dest.display());
            ensure!(result["regressions"].is_array(), "missing regression list");
            if !result["regressions"].as_array().unwrap().is_empty() {
                std::process::exit(3);
            }
            Ok(())
        }
    }
}
