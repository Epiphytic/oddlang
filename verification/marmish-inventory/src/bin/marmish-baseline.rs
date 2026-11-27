//! Observe the native Marmish baseline on Apple Silicon in an offline sandbox.
use anyhow::Result;
use clap::Parser;
use marmish_inventory::{PIN, baseline};
use std::path::PathBuf;
#[derive(Parser)]
struct Args {
    upstream: PathBuf,
    output: PathBuf,
}
fn main() -> Result<()> {
    let args = Args::parse();
    baseline::run(&args.upstream.canonicalize()?, &args.output, PIN, false)
}
