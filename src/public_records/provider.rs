//! Adapt the native projection and external components to the same validated inputs and outputs.
use super::*;
use crate::providers::{self, ProviderOutput, Registry};
use serde_json::{Value as Json, json};

pub fn build_selected(
    graph: &Path,
    overlay: &Path,
    out: &Path,
    registry: Option<&Registry>,
) -> Result<()> {
    let (output, receipt) = generate(
        &providers::read_input(graph)?,
        &providers::read_input(overlay)?,
        None,
        true,
        registry,
    )?;
    publish_profile(out, output, receipt, registry.is_some())
}

pub(super) fn publish_profile(
    out: &Path,
    output: ProviderOutput,
    receipt: Json,
    selected: bool,
) -> Result<()> {
    if selected {
        return providers::publish(out, &output, &receipt);
    }
    let files = output
        .artifacts
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    emit(out, &files)
}

pub(crate) fn generate(
    graph: &[u8],
    overlay: &[u8],
    source_api: Option<&[u8]>,
    serde_available: bool,
    registry: Option<&Registry>,
) -> Result<(ProviderOutput, Json)> {
    let model: Graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let mut binding: Overlay = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    if let Some(assertion) = source_api {
        binding.select_source_api(serde_json::from_slice(assertion).map_err(|e| e.to_string())?)?;
    }
    let records = validate(&model, &binding, serde_available)?;
    let mut exports = BTreeSet::new();
    let mut deps = BTreeSet::new();
    for module in &model.modules {
        if !binding.flatten_module {
            exports.insert(binding.name(&module.id).to_owned());
        }
        for record in &module.records {
            if binding.flatten_module {
                exports.insert(binding.name(&record.id).to_owned());
            }
            if record
                .traits
                .iter()
                .any(|t| matches!(t, Trait::Serialize | Trait::Deserialize))
            {
                deps.insert(String::from("serde"));
            }
        }
    }
    let dependencies: Vec<_> = deps.into_iter().collect();
    let fallback = Registry::default();
    let (mut output, receipt) = registry.unwrap_or(&fallback).run(
        "generate",
        "public-records",
        "1",
        json!({"graph":model, "overlay":binding, "serdeAvailable":serde_available}),
        &dependencies,
        || {
            Ok(ProviderOutput {
                artifacts: BTreeMap::from([(
                    "records.rs".into(),
                    render_validated(&model, &binding, &records).into_bytes(),
                )]),
                exports: exports.iter().cloned().collect(),
                dependencies: dependencies.clone(),
                gaps: vec![],
                evidence: vec![],
            })
        },
    )?;
    if output.artifacts.len() != 1
        || output.exports.iter().cloned().collect::<BTreeSet<_>>() != exports
        || output.dependencies.iter().collect::<BTreeSet<_>>() != dependencies.iter().collect()
    {
        return Err(
            "public record provider returned unexpected artifacts, exports or dependencies".into(),
        );
    }
    let code = output
        .artifacts
        .get("records.rs")
        .ok_or("public record provider omitted records.rs")?;
    syn::parse_file(std::str::from_utf8(code).map_err(|e| e.to_string())?)
        .map_err(|e| format!("invalid Rust projection: {e}"))?;
    let mut lock = json!({"format":"oddlang-public-record-projection-lock-v1","graphSha256":digest(graph),"overlaySha256":digest(overlay),"sourceSha256":digest(code)});
    if let Some(assertion) = source_api {
        lock["sourceApiSha256"] = json!(digest(assertion));
    }
    output
        .artifacts
        .insert("projection.lock".into(), encode(&lock)?);
    Ok((output, receipt))
}
