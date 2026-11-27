//! Report retained source modules and their pending portable conformance obligations.
use anyhow::{Context, Result, bail};
use clap::Parser;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};
const FRAME: &str = "crates/marmish-core/src/frame.rs";
const FACADE: &str = "crates/marmish-core/src/lib.rs";
#[derive(Parser)]
struct Args {
    /// Gzipped inventory, or '-' for JSON from stdin.
    inventory: PathBuf,
    /// Output JSON file, or '-' for stdout.
    output: PathBuf,
    #[arg(long, conflicts_with = "generated")]
    framing_only: bool,
    #[arg(long,num_args=0..)]
    generated: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    git_revision: String,
    symbols: Vec<Symbol>,
}
#[derive(Deserialize)]
struct Symbol {
    path: String,
    classification: String,
    targets: Vec<String>,
}
fn family(path: &str) -> Result<&'static str> {
    let parts: Vec<_> = Path::new(path).iter().collect();
    if parts.len() < 3 || parts[0] != "crates" {
        bail!("runtime source has no known first-party package: {path}");
    }
    Ok(
        match (parts[1].to_str(), parts.last().and_then(|p| p.to_str())) {
            (Some("marmish-core"), Some("lib.rs")) => "modules",
            _ if path == "crates/marmish-core/src/config.rs" => "config",
            (Some("marmish-stream"), Some("codec.rs")) => "compression",
            (Some("marmish-stream"), Some("windows.rs")) => "offset",
            (Some("marmish-stream"), Some("wire.rs" | "copy_protocol.rs")) => "wire",
            (Some("marmish-vault"), _) => "os",
            _ => "native",
        },
    )
}
fn register(model: Inventory, generated: BTreeSet<String>) -> Result<Value> {
    let mut files: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();
    for symbol in model.symbols {
        if symbol.classification == "retained-native" && !generated.contains(&symbol.path) {
            let (count, targets) = files.entry(symbol.path).or_default();
            *count += 1;
            targets.extend(symbol.targets);
        }
    }
    let mut needed = BTreeSet::new();
    let mut modules = Vec::new();
    for (path, (count, targets)) in files {
        let task = format!("urn:oddlang:work:marmish-capability-{}", family(&path)?);
        modules.push(json!({"path":path,"lexicalSites":count,"targets":targets,"status":"retained-native","blockedOn":task}));
        needed.insert(task);
    }
    let mut tasks: Vec<Value> = serde_json::from_str(include_str!("../../gap-tasks.json"))?;
    tasks.retain(|t| t["id"].as_str().is_some_and(|id| needed.contains(id)));
    let boundary = if generated.len() == 1 && generated.contains(FRAME) {
        "the framing module"
    } else {
        "the regenerated modules"
    };
    Ok(
        json!({"revision":model.git_revision,"scope":format!("Known runtime-source lexical declaration sites outside {boundary}. Macro expansions, lexical call targets and per-configuration compiled-item counts remain open."),"tasks":tasks,"modules":modules}),
    )
}
fn main() -> Result<()> {
    let args = Args::parse();
    let model = if args.inventory == Path::new("-") {
        serde_json::from_reader(io::stdin().lock())?
    } else {
        serde_json::from_reader(GzDecoder::new(
            File::open(&args.inventory).context("open inventory")?,
        ))?
    };
    let generated = if let Some(paths) = args.generated {
        paths
    } else if args.framing_only {
        vec![FRAME.into()]
    } else {
        vec![FRAME.into(), FACADE.into()]
    };
    let result = register(model, generated.into_iter().collect())?;
    let mut out: Box<dyn Write> = if args.output == Path::new("-") {
        Box::new(io::stdout().lock())
    } else {
        Box::new(File::create(args.output)?)
    };
    serde_json::to_writer_pretty(&mut out, &result)?;
    out.write_all(b"\n")?;
    Ok(())
}
