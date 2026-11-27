//! Collect or compare the pinned marmish-stream compiler input inventory.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use marmish_inventory::{read_json, stream_baseline};
use std::{fs, path::PathBuf};
#[derive(Parser)]
struct Args {
    workspace: PathBuf,
    output: PathBuf,
    #[arg(long)]
    expected: Option<PathBuf>,
    #[arg(long)]
    cargo_home: Option<PathBuf>,
    #[arg(long)]
    vendor: Option<PathBuf>,
    #[arg(long)]
    sandbox: Option<PathBuf>,
}
fn main() -> Result<()> {
    let argv: Vec<_> = std::env::args_os().skip(1).collect();
    if let [mode, original, candidate] = argv.as_slice()
        && mode == "compare"
    {
        return stream_baseline::compare(
            &read_json(original.as_ref())?,
            &read_json(candidate.as_ref())?,
        );
    }
    let args = Args::parse();
    let root = args.workspace.canonicalize()?;
    let output = std::path::absolute(&args.output)?;
    let parent = output
        .parent()
        .context("missing output parent")?
        .canonicalize()?;
    let output = parent.join(output.file_name().context("missing output name")?);
    ensure!(
        !output.starts_with(&root),
        "put receipt directory outside the workspace"
    );
    fs::create_dir(&output)?;
    let original = args.expected.as_deref().map(read_json).transpose()?;
    let home = args
        .cargo_home
        .or_else(|| std::env::var_os("CARGO_HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo"))
        .canonicalize()?;
    let sandbox = args.sandbox.map(fs::canonicalize).transpose()?;
    let vendor = args.vendor.map(fs::canonicalize).transpose()?;
    let report = stream_baseline::capture(
        &root,
        &output,
        original.as_ref(),
        &home,
        sandbox.as_deref(),
        vendor.as_deref(),
    )?;
    if let Some(old) = original {
        stream_baseline::compare(&old, &report)?;
    }
    let dest = output.join("receipt.json");
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    fs::write(&dest, bytes)?;
    println!("{}", dest.display());
    Ok(())
}
