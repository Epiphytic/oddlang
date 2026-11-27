//! Recover the pinned framing contract and its precedence mutant, then qualify both projections.
use clap::Parser;
use provider_validation::{Result, wasm_toolchain::run};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    upstream: PathBuf,
    output: PathBuf,
    #[arg(long)]
    oddlang: Option<PathBuf>,
    #[arg(long)]
    tools_cache: Option<PathBuf>,
    #[arg(long)]
    host: Option<PathBuf>,
}

fn compiler(repo: &Path) -> Result<PathBuf> {
    let result = Command::new("cargo")
        .current_dir(repo)
        .args([
            "+1.97.1",
            "build",
            "--offline",
            "--locked",
            "--bin",
            "oddlang",
            "--message-format=json-render-diagnostics",
        ])
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "could not build Oddlang: {}",
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    for line in result
        .stdout
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
    {
        let item: Value = serde_json::from_slice(line)?;
        if item["reason"] == "compiler-artifact"
            && item["target"]["name"] == "oddlang"
            && let Some(path) = item["executable"].as_str()
        {
            return Ok(PathBuf::from(path));
        }
    }
    Err("Cargo did not return an Oddlang executable".into())
}

fn project(repo: &Path, cli: &Path, source: &Path, import: &Path, projection: &Path) -> Result {
    let manifest = source.join("crates/marmish-core/Cargo.toml");
    let mapping = repo.join("examples/marmish/framing.toml");
    let offline = || {
        let mut cmd = Command::new(repo.join("scripts/marmish/offline.sh"));
        cmd.arg(cli);
        cmd
    };
    run(offline()
        .args(["import", "rust"])
        .arg(&manifest)
        .arg(&mapping)
        .arg(import))?;
    run(Command::new(cli).args(["import", "verify"]).arg(import))?;
    run(offline()
        .args(["import", "replay"])
        .arg(manifest)
        .arg(mapping)
        .arg(import))?;
    run(Command::new(cli)
        .args(["build", "module"])
        .arg(import)
        .arg(projection))?;
    run(Command::new(cli)
        .args(["import", "verify-build"])
        .arg(import)
        .arg(projection))
}

fn helper(name: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "+1.97.1",
        "run",
        "--locked",
        "--offline",
        "--quiet",
        "--manifest-path",
    ])
    .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
    .args(["--bin", name, "--"]);
    cmd
}

fn main() -> Result {
    let args = Args::parse();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let upstream = args.upstream.canonicalize()?;
    fs::create_dir_all(&args.output)?;
    let out = args.output.canonicalize()?;
    let cli = match args.oddlang {
        Some(p) => p.canonicalize()?,
        None => compiler(&repo)?,
    };
    let import = out.join("import");
    let projection = out.join("projection");
    project(&repo, &cli, &upstream, &import, &projection)?;
    let scratch = tempfile::tempdir()?;
    let mutant = scratch.path().join("mutant");
    run(Command::new("git")
        .args([
            "-c",
            "init.templateDir=",
            "clone",
            "--quiet",
            "--no-local",
            "--no-checkout",
        ])
        .arg(&upstream)
        .arg(&mutant))?;
    run(Command::new("git").arg("-C").arg(&mutant).args([
        "-c",
        "core.hooksPath=/dev/null",
        "checkout",
        "--quiet",
        "--detach",
        "49fc705408a230bd0ad896248d08dbdd2faad05a",
    ]))?;
    let path = mutant.join("crates/marmish-core/src/frame.rs");
    let source = fs::read_to_string(&path)?;
    let version = "        if envelope.version != 1 {\n            return Err(FrameError::InvalidVersion);\n        }\n";
    let size = "        let max_encoded = Limits::RAW_BYTES.div_ceil(3) * 4;\n        if envelope.payload.len() > max_encoded {\n            return Err(FrameError::SizeLimit);\n        }\n";
    let anchor = format!("{version}{size}");
    if source.matches(&anchor).count() != 1 {
        return Err("could not identify both original guards".into());
    }
    fs::write(
        path,
        source.replacen(&anchor, &format!("{size}{version}"), 1),
    )?;
    let mutant_projection = out.join("mutant-projection");
    project(
        &repo,
        &cli,
        &mutant,
        &out.join("mutant-import"),
        &mutant_projection,
    )?;
    let mut build = helper("framing-build");
    build
        .arg(&upstream)
        .arg(import)
        .arg(projection)
        .arg(out.join("observations"))
        .arg(mutant_projection);
    if let Some(cache) = args.tools_cache {
        build.arg("--tools-cache").arg(cache);
    }
    if let Some(host) = args.host {
        build.arg("--host").arg(host);
    }
    run(&mut build)?;
    run(helper("assembled-pilot").arg(upstream).arg(out).arg(cli))
}
