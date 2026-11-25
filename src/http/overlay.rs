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
        if (
            item.role.as_str(),
            item.target.as_str(),
            item.binding.as_str(),
        ) != ("http.provider", "rust", "axum")
        {
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
