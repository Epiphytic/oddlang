//! A workspace can compose a registered generator into a native Rust module.
use super::*;
use crate::providers::Registry;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    capability: String,
    version: String,
}

impl Selection {
    pub(super) fn generate(
        &self,
        registry: Option<&Registry>,
        component: &Component,
        graph: &[u8],
        overlay: &[u8],
        dependencies: &BTreeMap<String, Dependency>,
        output_dir: &Path,
    ) -> Result<Value> {
        let input = json!({
            "graph": crate::providers::document(graph)?,
            "overlay": crate::providers::document(overlay)?,
        });
        let registry = registry.ok_or("external package component requires a provider registry")?;
        let allowed = dependencies.keys().cloned().collect::<Vec<_>>();
        let (mut output, receipt) = registry.run(
            "generate",
            &self.capability,
            &self.version,
            input,
            &allowed,
            || Err(format!("no registered generator for {}", self.capability)),
        )?;
        let source = output
            .artifacts
            .remove("module.rs")
            .ok_or("Rust package provider must emit module.rs")?;
        if !output.artifacts.is_empty() {
            return Err("unexpected extra Rust package provider artifacts".into());
        }
        if component
            .reexports
            .iter()
            .chain(&component.crate_reexports)
            .any(|name| !output.exports.contains(name))
        {
            return Err("missing Rust package provider export declaration".into());
        }
        let lock = json!({"format":"oddlang-registered-projection-v1","capability":self.capability,"version":self.version,
            "sourceSha256":digest(&source),"graphSha256":digest(graph),"overlaySha256":digest(overlay)});
        io(fs::create_dir(output_dir))?;
        io(fs::write(output_dir.join("module.rs"), source))?;
        io(fs::write(
            output_dir.join("projection.lock"),
            serde_json::to_vec(&lock).map_err(|e| e.to_string())?,
        ))?;
        Ok(receipt)
    }
}
