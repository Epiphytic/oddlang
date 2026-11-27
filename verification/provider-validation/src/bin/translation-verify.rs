//! Recover and regenerate the ledger, compare independent native callers, and qualify the Kani proof.
use clap::Parser;
use provider_validation::{Result, sha};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

// The ledger-v1 import profile and its native callers use this compiler.
const LEDGER_TOOLCHAIN: &str = "1.96.1";

#[derive(Parser)]
struct Args {
    /// Empty report destination. By default a new directory is allocated under target/.
    output: Option<PathBuf>,
    /// Use an existing Oddlang executable for import and generation.
    #[arg(long)]
    oddlang: Option<PathBuf>,
    /// Use an already compiled rust_translation test binary.
    #[arg(long)]
    differential: Option<PathBuf>,
}

fn run(cmd: &mut Command, root: &Path) -> Result {
    let status = cmd
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .status()?;
    if !status.success() {
        return Err(format!("{cmd:?}: {status}").into());
    }
    Ok(())
}

fn tool(name: &str, test: bool) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args([
        if test { "test" } else { "run" },
        "--quiet",
        "--offline",
        "--locked",
        "--manifest-path",
    ])
    .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
    .args(["--bin", name, "--"]);
    cmd
}

fn source_pins(fixture: &Path) -> Result<BTreeMap<String, String>> {
    let mut pins = BTreeMap::new();
    for line in fs::read_to_string(fixture.join("original.sha256"))?.lines() {
        let (pin, file) = line
            .split_once("  ")
            .ok_or("invalid original.sha256 entry")?;
        if sha(&fixture.join(file))? != pin
            || pins.insert(file.to_owned(), pin.to_owned()).is_some()
        {
            return Err(format!("duplicate or mismatching source pin: {file}").into());
        }
    }
    Ok(pins)
}

fn main() -> Result {
    let args = Args::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let report = if let Some(dir) = args.output {
        fs::create_dir_all(&dir)?;
        if fs::read_dir(&dir)?.next().is_some() {
            return Err("translation evidence needs an empty output directory".into());
        }
        dir.canonicalize()?
    } else {
        let target = root.join("target");
        fs::create_dir_all(&target)?;
        tempfile::Builder::new()
            .prefix("translation.")
            .tempdir_in(target)?
            .keep()
    };
    eprintln!("Translation evidence directory: {}", report.display());
    let fixture = root.join("examples/rust-import/ledger");
    let pins = source_pins(&fixture)?;
    let oddlang = args.oddlang.map(fs::canonicalize).transpose()?;
    let cli = || {
        let mut cmd = if let Some(binary) = &oddlang {
            Command::new(binary)
        } else {
            let mut cargo = Command::new("cargo");
            cargo.args([
                "run",
                "--quiet",
                "--locked",
                "--offline",
                "--bin",
                "oddlang",
                "--",
            ]);
            cargo
        };
        cmd.env("RUSTUP_TOOLCHAIN", LEDGER_TOOLCHAIN);
        cmd
    };
    let original = report.join("original.pin");
    run(
        cli()
            .args(["cleanup", "freeze"])
            .arg(&fixture)
            .arg(&original),
        &root,
    )?;
    if fs::read(original)? != fs::read(root.join("examples/rust-import/ledger.baseline"))? {
        return Err("ledger baseline pin changed".into());
    }
    let import = report.join("import");
    for mode in ["rust", "replay"] {
        run(
            cli()
                .args(["import", mode])
                .arg(fixture.join("Cargo.toml"))
                .arg(fixture.join("mapping.toml"))
                .arg(&import),
            &root,
        )?;
    }
    let generated = report.join("generated");
    run(cli().arg("build").arg(&import).arg(&generated), &root)?;
    run(
        cli()
            .args(["import", "verify-build"])
            .arg(&import)
            .arg(&generated),
        &root,
    )?;
    let mut differential = if let Some(path) = args.differential {
        Command::new(path.canonicalize()?)
    } else {
        let mut cmd = Command::new("cargo");
        cmd.args([
            "test",
            "--locked",
            "--offline",
            "--test",
            "rust_translation",
        ]);
        cmd
    };
    run(
        differential
            .env("RUSTUP_TOOLCHAIN", LEDGER_TOOLCHAIN)
            .env("ODDLANG_TRANSLATION_ARTIFACTS", report.join("corpus"))
            .env(
                "ODDLANG_TRANSLATION_REPORT",
                report.join("differential.json"),
            ),
        &root,
    )?;
    let output = generated.join("account.rs");
    run(
        tool("translation-proof", false)
            .arg(fixture.join("src/lib.rs"))
            .arg(&output)
            .arg(
                pins.get("src/lib.rs")
                    .ok_or("missing original source pin")?,
            )
            .arg(sha(&output)?)
            .arg(report.join("proof")),
        &root,
    )?;
    run(
        tool("translation-evidence", false)
            .arg(&root)
            .arg(&report)
            .arg("--write"),
        &root,
    )?;
    run(
        tool("translation-evidence", false).arg(&root).arg(&report),
        &root,
    )?;
    run(
        tool("translation-evidence", true)
            .env("ODDLANG_REPLAY_FIXTURE", &report)
            .arg("--include-ignored"),
        &root,
    )?;
    println!("Verified Rust translation evidence in {}", report.display());
    Ok(())
}
