//! Preserve the 120-second subprocess deadline and exact stdout/stderr observations.
use provider_validation::{Result, pin, write};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

pub fn run(program: &Path, cwd: &Path, args: &[&str]) -> Result<Output> {
    provider_validation::subprocess::run(
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env("RUSTUP_TOOLCHAIN", "1.97.1")
            .env("CARGO_NET_OFFLINE", "true")
            .env_remove("CARGO_TARGET_DIR"),
        None,
    )
}
pub fn success(output: &Output) -> Result {
    if !output.status.success() {
        return Err(format!(
            "{}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}
pub fn cargo(cwd: &Path, args: &[&str]) -> Result {
    success(&run(Path::new("cargo"), cwd, args)?)
}
pub fn pinned(root: &Path, name: &str, obj: &Value) -> Result<Value> {
    write(&root.join(name), obj)?;
    pin(root, name)
}
pub fn build(
    cli: &Path,
    root: &Path,
    kind: &str,
    manifest: &str,
    destination: &str,
    error: Option<&str>,
) -> Result {
    let got = run(cli, root, &["build", kind, "rust", manifest, destination])?;
    if let Some(error) = error {
        assert!(
            !got.status.success() && String::from_utf8_lossy(&got.stderr).contains(error),
            "{got:?}"
        );
        assert!(!root.join(destination).exists());
        Ok(())
    } else {
        success(&got)
    }
}
