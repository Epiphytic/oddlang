//! Reassemble the original framing pilot and exercise its independent native callers.
use clap::Parser;
use provider_validation::Result;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Parser)]
struct Args {
    upstream: PathBuf,
    /// Contains import/ and projection/ from the framing recovery.
    evidence: PathBuf,
    oddlang: PathBuf,
}

fn checked(cmd: &mut Command) -> Result {
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("{cmd:?}: {status}").into());
    }
    Ok(())
}

fn helper(name: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "+1.97.1",
        "run",
        "--quiet",
        "--locked",
        "--offline",
        "--manifest-path",
    ])
    .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
    .args(["--bin", name, "--"]);
    cmd
}

fn main() -> Result {
    let args = Args::parse();
    let upstream = args.upstream.canonicalize()?;
    let out = args.evidence.canonicalize()?;
    let cli = args.oddlang.canonicalize()?;
    let scratch = tempfile::tempdir()?;
    let package = scratch.path().join("package");
    checked(
        helper("marmish-prepare")
            .arg("v1")
            .arg(&upstream)
            .arg(out.join("import"))
            .arg(out.join("projection"))
            .arg(&package),
    )?;
    let manifest = package.join("project.toml");
    let store = package.join("store");
    let candidate = scratch.path().join("candidate");
    let assemble = |dst: &Path| {
        let mut cmd = Command::new(&cli);
        cmd.arg("assemble").arg(&manifest).arg(&store).arg(dst);
        cmd
    };
    checked(&mut assemble(&candidate))?;
    fs::copy(&manifest, out.join("assembly.toml"))?;
    let log = fs::File::create(out.join("upstream-contracts.log"))?;
    checked(
        provider_validation::offline::command()?
            .args([
                "cargo",
                "+1.97.1",
                "test",
                "--locked",
                "--offline",
                "--manifest-path",
            ])
            .arg(candidate.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(scratch.path().join("native-tests"))
            .args(["-p", "marmish-core", "--test", "contracts"])
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log)),
    )?;
    checked(
        helper("build-closure")
            .arg(&candidate)
            .arg(&manifest)
            .arg(out.join("build-closure")),
    )?;

    let original = Command::new("git")
        .arg("-C")
        .arg(upstream)
        .args([
            "show",
            "49fc705408a230bd0ad896248d08dbdd2faad05a:crates/marmish-core/src/config.rs",
        ])
        .output()?;
    if !original.status.success() {
        return Err(String::from_utf8_lossy(&original.stderr)
            .into_owned()
            .into());
    }
    let hash = format!("{:x}", Sha256::digest(&original.stdout));
    fs::OpenOptions::new()
        .append(true)
        .open(store.join("retained").join(&hash))?
        .write_all(b"\n")?;
    let poisoned = scratch.path().join("poisoned");
    let failure = assemble(&poisoned).output()?;
    let message = format!("digest mismatch for retained/{hash}");
    let mut log = fs::File::create(out.join("poisoned-retained.log"))?;
    log.write_all(&failure.stdout)?;
    log.write_all(&failure.stderr)?;
    if failure.status.success()
        || poisoned.try_exists()?
        || !String::from_utf8_lossy(&failure.stderr).contains(&message)
    {
        return Err(format!("expected unpublished output and diagnostic {message}").into());
    }
    Ok(())
}
