//! Shared evidence readers for historical Marmish qualification.
pub mod baseline;
pub mod candidate;
pub mod stream_baseline;
pub mod stream_observer;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
pub type Map = serde_json::Map<String, Value>;
pub type Set = BTreeSet<String>;
pub const PIN: &str = "49fc705408a230bd0ad896248d08dbdd2faad05a";
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn read(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        fs::symlink_metadata(path).is_ok_and(|m| m.is_file()),
        "missing or redirected evidence: {}",
        path.display()
    );
    fs::read(path).with_context(|| format!("read {}", path.display()))
}
pub fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(&read(path)?).with_context(|| format!("invalid JSON {}", path.display()))
}
pub fn text(value: &Value) -> Result<&str> {
    value.as_str().context("expected string")
}
pub fn object(value: &Value) -> Result<&Map> {
    value.as_object().context("expected object")
}
pub fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().context("expected array")
}
pub fn strings(value: &Value) -> Result<Vec<String>> {
    array(value)?
        .iter()
        .map(|v| text(v).map(str::to_owned))
        .collect()
}
pub fn keys(value: &Value) -> Result<Set> {
    Ok(object(value)?.keys().cloned().collect())
}
pub fn set(names: &[&str]) -> Set {
    names.iter().map(|s| s.to_string()).collect()
}
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(encoded, "{b:02x}").expect("string");
    }
    encoded
}
pub fn hex_decode(hex: &str) -> Result<Vec<u8>> {
    let mut hex = hex.as_bytes();
    let mut bytes = vec![];
    while !hex.is_empty() {
        if hex[0].is_ascii_whitespace() {
            hex = &hex[1..];
            continue;
        }
        let pair = hex.get(..2).context("incomplete hex byte")?;
        bytes.push(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?);
        hex = &hex[2..];
    }
    Ok(bytes)
}
pub fn valid_hash(value: &Value, n: usize) -> bool {
    value
        .as_str()
        .is_some_and(|s| s.len() == n && s.bytes().all(|b| matches!(b,b'0'..=b'9'|b'a'..=b'f')))
}
pub fn filenames(path: &Path) -> Result<Set> {
    fs::read_dir(path)?
        .map(|item| {
            item?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid path"))
        })
        .collect()
}
pub fn file_hashes(dir: &Path, names: &[&str]) -> Result<Value> {
    let hashes: Map = names
        .iter()
        .map(|&name| Ok((name.into(), json!(digest(&read(&dir.join(name))?)))))
        .collect::<Result<_>>()?;
    Ok(json!(hashes))
}
pub fn run_json(command: &mut Command) -> Result<Value> {
    let output = command
        .output()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(
        output.status.success(),
        "command {command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}
