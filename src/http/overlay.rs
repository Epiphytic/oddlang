use super::HttpError;
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetOverlay {
    pub schema_version: u32,
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub hints: Vec<Hint>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub role: String,
    pub target: String,
    pub binding: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hint {
    pub role: String,
    pub target: String,
    pub hint: String,
}

pub fn load_overlay(path: &Path) -> Result<TargetOverlay, HttpError> {
    let overlay: TargetOverlay =
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if overlay.schema_version != 1 || overlay.bindings.is_empty() {
        return Err("expected schemaVersion 1 and at least one required binding".into());
    }
    let mut pairs = BTreeSet::new();
    for item in &overlay.bindings {
        if !matches!(
            (
                item.role.as_str(),
                item.target.as_str(),
                item.binding.as_str(),
            ),
            ("http.provider", "rust", "axum")
                | ("http.consumer", "typescript", "fetch")
                | ("json.codec", "rust", "serde")
                | ("json.codec", "typescript", "json")
        ) {
            return Err(format!(
                "unsupported required binding {}/{}/{}",
                item.role, item.target, item.binding
            ));
        }
        if !pairs.insert((&item.role, &item.target)) {
            return Err("duplicate binding role and target".into());
        }
    }
    if overlay
        .hints
        .iter()
        .any(|h| !pairs.contains(&(&h.role, &h.target)))
    {
        return Err("hint needs a matching role and target binding".into());
    }
    Ok(overlay)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetLock {
    schema_version: u32,
    contract: crate::contracts::Pin,
    case_corpus_sha256: String,
    source: ReviewedFile,
    overlay: Option<ReviewedFile>,
    toolchains: std::collections::BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedFile {
    path: String,
    sha256: String,
}

/// Replay the independent Go adapter's release, reviewed files and executable toolchain pins.
pub fn verify_target_lock(
    adapter: &Path,
    path: &Path,
    bundle: &Path,
) -> Result<super::VerifiedBundle, HttpError> {
    let lock: TargetLock = super::read_json(path)?;
    if lock.schema_version != 1 || lock.toolchains.len() != 1 {
        return Err("unsupported target lock schema or toolchains".into());
    }
    let expected = lock
        .toolchains
        .get("go")
        .ok_or("missing Go toolchain pin")?;
    let go = std::process::Command::new("go")
        .args(["env", "GOVERSION"])
        .env("GOTOOLCHAIN", "local")
        .output()
        .map_err(|e| format!("go version: {e}"))?;
    if !go.status.success() || String::from_utf8_lossy(&go.stdout).trim() != expected {
        return Err(format!("expected Go toolchain {expected}"));
    }
    for file in std::iter::once(&lock.source).chain(lock.overlay.as_ref()) {
        crate::contracts::verified(adapter, &file.path, &file.sha256)?;
    }
    let release = super::bundle::verify_pins(bundle, &super::pins(vec![lock.contract])?)?;
    crate::contracts::verified(bundle, "cases.json", &lock.case_corpus_sha256)?;
    Ok(release)
}
