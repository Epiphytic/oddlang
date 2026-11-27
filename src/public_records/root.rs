//! A distinct root-record profile with no generated module wrapper.
use super::*;
use crate::providers::{self, ProviderOutput, Registry};
use crate::root_data_scope as scope;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    source_sha256: String,
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, String>,
}

pub fn recover_root(source: &Path, selection: &Path, out: &Path) -> Result<()> {
    recover_root_selected(source, selection, out, None)
}

pub fn recover_root_selected(
    source: &Path,
    selection: &Path,
    out: &Path,
    registry: Option<&Registry>,
) -> Result<()> {
    let raw = providers::read_input(selection)?;
    let source_bytes = providers::read_input(source)?;
    let selected: Selection = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let syntax_candidate = selected.format == "oddlang-rust-selected-record-selection-v1";
    if (!syntax_candidate && selected.format != "oddlang-rust-root-record-selection-v1")
        || selected.records.is_empty()
    {
        return Err("invalid root public record selection".into());
    }
    if digest(&source_bytes) != selected.source_sha256 {
        return Err(format!(
            "public record source digest mismatch: {}",
            source.display()
        ));
    }
    let fallback = Registry::default();
    let input = json!({
        "selection": selected,
        "sources": [{"path": "source.rs", "base64": STANDARD.encode(&source_bytes)}]
    });
    let (mut output, receipt) =
        registry
            .unwrap_or(&fallback)
            .run("recover", "public-records", "1", input, &[], || {
                recover_bytes(&source_bytes, &selected, syntax_candidate)
            })?;
    if output.artifacts.len() != 2
        || output.evidence != ["source.rs"]
        || output.gaps.is_empty()
        || !output.exports.is_empty()
    {
        return Err("invalid root public record recovery result or evidence".into());
    }
    let graph_bytes = output
        .artifacts
        .get("graph.json")
        .ok_or("provider omitted graph.json")?;
    let overlay_bytes = output
        .artifacts
        .get("rust.json")
        .ok_or("provider omitted rust.json")?;
    let graph: Graph = serde_json::from_slice(graph_bytes).map_err(|e| e.to_string())?;
    let overlay: Overlay = serde_json::from_slice(overlay_bytes).map_err(|e| e.to_string())?;
    validate(&graph, &overlay, true)?;
    if !overlay.flatten_module
        || graph.modules[0].id != selected.id
        || graph.modules[0]
            .records
            .iter()
            .map(|r| (overlay.name(&r.id), &r.id))
            .collect::<BTreeMap<_, _>>()
            != selected
                .records
                .iter()
                .map(|(k, v)| (k.as_str(), v))
                .collect()
    {
        return Err("provider recovered declarations outside the selected root records".into());
    }
    let lock = encode(&json!({
        "format": "oddlang-root-public-record-recovery-lock-v1",
        "selectionSha256": digest(&raw), "crateSourceSha256": selected.source_sha256,
        "graphSha256": digest(graph_bytes), "overlaySha256": digest(overlay_bytes),
        "gap": output.gaps.join("; "),
    }))?;
    output.artifacts.insert("recovery.lock".into(), lock);
    provider::publish_profile(out, output, receipt, registry.is_some())
}

fn recover_bytes(
    bytes: &[u8],
    selected: &Selection,
    syntax_candidate: bool,
) -> Result<ProviderOutput> {
    let rust = syn::parse_file(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
        .map_err(|e| format!("invalid Rust source: {e}"))?;
    scope::ordinary_root(&rust)?;
    let names: Vec<_> = selected.records.keys().map(String::as_str).collect();
    if syntax_candidate {
        // The selected declarations are explicit. Sibling item macro effects are verified
        // separately by a consumer compiling against the original and projected crates.
        let mut declarations = rust.clone();
        declarations
            .items
            .retain(|item| !matches!(item, syn::Item::Macro(_)));
        scope::bindings(&declarations, &names)?;
        recover::reject_item_expansion(&declarations)?;
    } else {
        scope::bindings(&rust, &names)?;
        recover::reject_item_expansion(&rust)?;
    }
    let imports = recover::imports(&rust);
    if imports.iter().any(|item| item.name == "*") {
        return Err("glob imports are unsupported in root public records".into());
    }
    recover::reject_type_shadows(&rust, &imports)?;
    let mut module = Module {
        id: selected.id.clone(),
        records: vec![],
        aliases: vec![],
    };
    let mut identifiers = BTreeMap::from([(selected.id.clone(), "root".into())]);
    let mut found = BTreeSet::new();
    for item in &rust.items {
        if let syn::Item::Struct(item) = item {
            let name = item.ident.to_string();
            if let Some(identity) = selected.records.get(&name) {
                if !found.insert(name.clone())
                    || identifiers.insert(identity.clone(), name).is_some()
                {
                    return Err("duplicate root public record binding".into());
                }
                module
                    .records
                    .push(recover::record(item, identity, &imports)?);
            }
        }
    }
    if module.records.len() != selected.records.len() {
        return Err("missing selected root public record".into());
    }
    let graph = Graph {
        format: GRAPH.into(),
        modules: vec![module],
    };
    let overlay = Overlay {
        format: OVERLAY.into(),
        flatten_module: true,
        identifiers,
        external_types: BTreeMap::new(),
        source_api: None,
    };
    validate(&graph, &overlay, true)?;
    let graph = encode(&graph)?;
    let overlay = encode(&overlay)?;
    let gap = if syntax_candidate {
        "Sibling item macro expansion, Rust bindings and public export paths require independent native compilation"
    } else {
        "Rust derive and type bindings require downstream native compilation"
    };
    Ok(ProviderOutput {
        artifacts: BTreeMap::from([("graph.json".into(), graph), ("rust.json".into(), overlay)]),
        exports: vec![],
        dependencies: vec![],
        evidence: vec!["source.rs".into()],
        gaps: vec![gap.into()],
    })
}
