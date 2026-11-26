//! A distinct root-record profile with no generated module wrapper.
use super::*;
use crate::root_data_scope as scope;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    source_sha256: String,
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, String>,
}

pub fn recover_root(source: &Path, selection: &Path, out: &Path) -> Result<()> {
    let raw = io(fs::read(selection))?;
    let selected: Selection = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let syntax_candidate = selected.format == "oddlang-rust-selected-record-selection-v1";
    if (!syntax_candidate && selected.format != "oddlang-rust-root-record-selection-v1")
        || selected.records.is_empty()
    {
        return Err("invalid root public record selection".into());
    }
    let rust = recover::select_source(source, &selected.source_sha256)?;
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
    };
    render(&graph, &overlay, true)?;
    let graph = encode(&graph)?;
    let overlay = encode(&overlay)?;
    let gap = if syntax_candidate {
        "Sibling item macro expansion, Rust bindings and public export paths require independent native compilation"
    } else {
        "Rust derive and type bindings require downstream native compilation"
    };
    let lock = encode(&serde_json::json!({
        "format": "oddlang-root-public-record-recovery-lock-v1",
        "selectionSha256": digest(&raw), "crateSourceSha256": selected.source_sha256,
        "graphSha256": digest(&graph), "overlaySha256": digest(&overlay),
        "gap": gap,
    }))?;
    emit(
        out,
        &[
            ("graph.json", &graph),
            ("rust.json", &overlay),
            ("recovery.lock", &lock),
        ],
    )
}
