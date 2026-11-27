//! Acquire the pinned Marmish sources and warm the native and ontology-tool caches.
use anyhow::{Context, Result, ensure};
use marmish_inventory::PIN;
use std::{fs, path::Path, process::Command};

const UPSTREAM: &str = "https://github.com/Epiphytic/marmish.git";

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("start {cmd:?}"))?;
    ensure!(status.success(), "{cmd:?}: {status}");
    Ok(())
}

fn pinned(dir: &Path) -> Result<()> {
    let actual = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(
        actual.status.success() && actual.stdout.strip_suffix(b"\n") == Some(PIN.as_bytes()),
        "expected Marmish revision {PIN} in {}",
        dir.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    let root = marmish_inventory::root();
    let upstream = marmish_inventory::upstream_path(None);
    if !upstream.try_exists()? {
        let parent = upstream
            .parent()
            .context("checkout needs a parent directory")?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::tempdir_in(parent)?;
        let checkout = staging.path().join("upstream");
        run(Command::new("git")
            .args(["clone", "--no-checkout", UPSTREAM])
            .arg(&checkout))?;
        run(Command::new("git")
            .arg("-C")
            .arg(&checkout)
            .args(["checkout", "--detach", PIN]))?;
        pinned(&checkout)?;
        fs::rename(checkout, &upstream)?;
    }
    pinned(&upstream)?;
    run(Command::new("rustc").current_dir(&upstream).arg("-Vv"))?;
    run(Command::new("cargo")
        .current_dir(&upstream)
        .args(["fetch", "--locked"]))?;
    // This indexer and its Python environment belong to the pinned upstream checkout.
    run(Command::new("uv").current_dir(&upstream).args([
        "sync",
        "--locked",
        "--python",
        "3.12",
        "--project",
        "tools/code-ontology",
    ]))?;
    let framing = "examples/wasm/framing/Cargo.toml";
    for (manifest, target) in [
        ("examples/rust-import/framing-independent/Cargo.toml", None),
        (framing, None),
        (framing, Some("wasm32-unknown-unknown")),
    ] {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&root)
            .args(["+1.97.1", "fetch", "--locked", "--manifest-path", manifest]);
        if let Some(target) = target {
            cmd.args(["--target", target]);
        }
        run(&mut cmd)?;
    }
    Ok(())
}
