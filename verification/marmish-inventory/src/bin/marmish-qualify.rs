//! Public qualification entry point for a clean, revision-pinned checkout.
use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use marmish_inventory::{PIN, offline};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Baseline,
    Frame,
    Rebuild,
    Verify,
}

#[derive(Parser)]
struct Args {
    mode: Mode,
    upstream: Option<PathBuf>,
    output: Option<PathBuf>,
    /// Use an already built compiler for frame or rebuild.
    #[arg(long)]
    oddlang: Option<PathBuf>,
}

fn check_source(src: &Path) -> Result<()> {
    let git = |args: &[&str]| -> Result<Vec<u8>> {
        let out = Command::new("git").arg("-C").arg(src).args(args).output()?;
        ensure!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(out.stdout)
    };
    ensure!(
        String::from_utf8(git(&["rev-parse", "HEAD"])?)?.trim() == PIN,
        "Marmish revision mismatch"
    );
    ensure!(
        git(&["diff", "--name-only", "-z", "HEAD", "--"])?.is_empty(),
        "Marmish tracked inputs differ from the pinned tree"
    );
    ensure!(
        git(&["ls-files", "--others", "--exclude-standard", "-z"])?.is_empty(),
        "Untracked Marmish inputs present"
    );
    Ok(())
}

struct Gate {
    root: PathBuf,
    upstream: PathBuf,
    out: PathBuf,
}

impl Gate {
    fn child(&self, bin: &str, isolated: bool) -> Result<Command> {
        let mut cmd = if isolated {
            let mut cmd = offline::command()?;
            cmd.arg("cargo");
            cmd
        } else {
            Command::new("cargo")
        };
        let manifest = if bin == "project-frame" {
            "verification/provider-validation/Cargo.toml"
        } else {
            "verification/marmish-inventory/Cargo.toml"
        };
        cmd.current_dir(&self.root)
            .env("CARGO_NET_OFFLINE", "true")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env("UV_OFFLINE", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .args([
                "+1.97.1",
                "run",
                "--quiet",
                "--locked",
                "--offline",
                "--manifest-path",
                manifest,
                "--bin",
                bin,
                "--",
            ]);
        Ok(cmd)
    }

    fn run(&self, mode: Mode, compiler: Option<&Path>) -> Result<()> {
        match mode {
            Mode::Baseline => {
                checked(
                    self.child("marmish-inventory", true)?
                        .arg(&self.upstream)
                        .arg(self.out.join("inventory"))
                        .args([
                            "--compare-summary",
                            "examples/marmish/inventory-summary.json",
                        ]),
                )?;
                checked(
                    self.child("framing-observer", true)?
                        .arg("collect")
                        .arg(&self.upstream)
                        .arg(self.out.join("observations")),
                )?;
                checked(
                    self.child("marmish-baseline", false)?
                        .arg(&self.upstream)
                        .arg(self.out.join("native")),
                )?;
                checked(
                    self.child("marmish-receipt", false)?
                        .arg("check")
                        .arg(&self.out)
                        .arg("--write"),
                )?;
                checked(
                    self.child("marmish-receipt", false)?
                        .arg("check")
                        .arg(&self.out),
                )?;
                println!("Marmish baseline recorded in {}", self.out.display());
            }
            Mode::Frame | Mode::Rebuild | Mode::Verify => {
                let bin = match mode {
                    Mode::Frame => "project-frame",
                    Mode::Rebuild => "marmish-rebuild",
                    _ => "marmish-phase7",
                };
                let mut cmd = self.child(bin, false)?;
                cmd.arg(&self.upstream).arg(&self.out);
                if let Some(path) = compiler {
                    cmd.arg("--oddlang").arg(path);
                }
                checked(&mut cmd)?;
                println!("Marmish qualification evidence in {}", self.out.display());
            }
        }
        Ok(())
    }
}

fn checked(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("start {cmd:?}"))?;
    if !status.success() {
        eprintln!("{cmd:?}: {status}");
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::exit(
                status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
            );
        }
        #[cfg(not(unix))]
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.oddlang.is_none() || matches!(args.mode, Mode::Frame | Mode::Rebuild),
        "--oddlang requires frame or rebuild"
    );
    let compiler = args.oddlang.map(fs::canonicalize).transpose()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let cache = root.join("target/marmish");
    let upstream = args
        .upstream
        .or_else(|| env::var_os("ODDLANG_MARMISH_SOURCE").map(PathBuf::from))
        .unwrap_or_else(|| cache.join("upstream"))
        .canonicalize()?;
    check_source(&upstream)?;
    let out = if let Some(out) = args.output {
        fs::create_dir_all(&out)?;
        ensure!(
            fs::read_dir(&out)?.next().is_none(),
            "Output directory must be empty"
        );
        out.canonicalize()?
    } else {
        fs::create_dir_all(&cache)?;
        let prefix = format!("{}.", args.mode.to_possible_value().unwrap().get_name());
        tempfile::Builder::new()
            .prefix(&prefix)
            .tempdir_in(cache)?
            .keep()
    };
    Gate {
        root,
        upstream,
        out,
    }
    .run(args.mode, compiler.as_deref())
}
