//! Helpers for the public record qualification binaries.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

pub fn cli() -> Result<PathBuf> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [path] = args.as_slice() else {
        return Err("expected one Oddlang executable path".into());
    };
    Ok(Path::new(path).canonicalize()?)
}
pub fn sha(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
pub fn pin(root: &Path, file: &str) -> Result<Value> {
    Ok(json!({"path":file,"sha256":sha(&root.join(file))?}))
}
pub fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn write(path: &Path, value: &Value) -> Result {
    Ok(fs::write(path, serde_json::to_vec(value)?)?)
}

pub fn command(
    command: &mut Command,
    cwd: &Path,
    args: &[&str],
    error: Option<&str>,
) -> Result<Vec<u8>> {
    command
        .current_dir(cwd)
        .args(args)
        .env("RUSTUP_TOOLCHAIN", "1.97.1")
        .env("CARGO_NET_OFFLINE", "true");
    let result = command.output()?;
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    match error {
        Some(expected) if !result.status.success() && diagnostic.contains(expected) => (),
        None if result.status.success() => (),
        _ => {
            return Err(format!(
                "{command:?}: expected {error:?}, got {}: {diagnostic}",
                result.status
            )
            .into());
        }
    }
    Ok(result.stdout)
}
pub fn cargo(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    command(&mut Command::new("cargo"), root, args, None)
}
pub fn invoke(cli: &Path, root: &Path, args: &[&str], output: &str, error: Option<&str>) -> Result {
    let mut args = args.to_vec();
    args.push(output);
    command(&mut Command::new(cli), root, &args, error)?;
    if root.join(output).exists() == error.is_some() {
        return Err(format!("unexpected output directory state: {output}").into());
    }
    Ok(())
}
