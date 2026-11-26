//! Test-only structural parsing of bounded local JSON values.
use crate::{
    shared_declarations::unique_map,
    wire_schema::{digest, encode, iri, symbol},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Contract {
    format: String,
    id: String,
    maximum_value_bytes: u32,
    maximum_records: u32,
    framing: String,
    size_boundary: String,
    duplicate_members: String,
    guard_order: Vec<String>,
    capacity_error: String,
    invalid_error: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    single: String,
    sequence: String,
    limit: String,
    error_type: String,
    #[serde(deserialize_with = "unique_map")]
    variants: BTreeMap<String, String>,
}

fn parse(graph: &[u8], overlay: &[u8]) -> Result<(Contract, Binding)> {
    let graph: Contract = serde_json::from_slice(graph).map_err(|error| error.to_string())?;
    let binding: Binding = serde_json::from_slice(overlay).map_err(|error| error.to_string())?;
    for id in [&graph.id, &graph.capacity_error, &graph.invalid_error] {
        iri(id)?;
    }
    if graph.format != "oddlang-bounded-json-input-v1"
        || graph.maximum_value_bytes == 0
        || graph.maximum_records == 0
        || graph.framing != "lf-terminated"
        || graph.size_boundary != "include-delimiter"
        || graph.duplicate_members != "last-value"
        || graph.guard_order != ["byte-limit", "record-limit", "parse"]
        || graph.capacity_error == graph.invalid_error
        || [&graph.capacity_error, &graph.invalid_error].contains(&&graph.id)
    {
        return Err("unsupported bounded JSON contract".into());
    }
    if binding.format != "oddlang-bounded-json-rust-v1"
        || [&binding.single, &binding.sequence, &binding.limit]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != 3
        || ![
            &binding.single,
            &binding.sequence,
            &binding.limit,
            &binding.error_type,
        ]
        .into_iter()
        .chain(binding.variants.values())
        .all(|name| symbol(name))
        || binding.variants.len() != 2
        || !binding.variants.contains_key(&graph.capacity_error)
        || !binding.variants.contains_key(&graph.invalid_error)
        || binding.variants[&graph.capacity_error] == binding.variants[&graph.invalid_error]
    {
        return Err("invalid bounded JSON native binding".into());
    }
    Ok((graph, binding))
}

pub(crate) fn error_paths(graph: &[u8], overlay: &[u8]) -> Result<Vec<(String, String)>> {
    let (_, binding) = parse(graph, overlay)?;
    Ok(binding
        .variants
        .into_iter()
        .map(|(id, name)| (id, format!("crate::{}::{name}", binding.error_type)))
        .collect())
}

pub(crate) fn build_bytes(graph: &[u8], overlay: &[u8], out: &Path) -> Result<()> {
    let (contract, binding) = parse(graph, overlay)?;
    let code = include_str!("rust.rs.txt")
        .replace("@SINGLE@", &binding.single)
        .replace("@SEQUENCE@", &binding.sequence)
        .replace("@LIMIT@", &binding.limit)
        .replace("@ERROR@", &binding.error_type)
        .replace("@CAPACITY@", &binding.variants[&contract.capacity_error])
        .replace("@INVALID@", &binding.variants[&contract.invalid_error])
        .replace("@BYTES@", &contract.maximum_value_bytes.to_string())
        .replace("@RECORDS@", &contract.maximum_records.to_string());
    fs::create_dir(out).map_err(|e| e.to_string())?;
    let lock = encode(&serde_json::json!({
        "format": "oddlang-bounded-json-input-lock-v1",
        "graphSha256": digest(graph), "overlaySha256": digest(overlay),
        "sourceSha256": digest(code.as_bytes()),
    }))?;
    fs::write(out.join("metadata.rs"), code).map_err(|e| e.to_string())?;
    fs::write(out.join("projection.lock"), lock).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    format: String,
    id: String,
    observation_sha256: String,
    capacity_error: String,
    invalid_error: String,
    binding: Binding,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observation {
    format: String,
    accepted_value_bytes: u32,
    rejected_value_bytes: u32,
    accepted_records: u32,
    rejected_records: u32,
    #[serde(deserialize_with = "unique_map")]
    cases: BTreeMap<String, String>,
}

pub fn recover(observation: &Path, selection: &Path, out: &Path) -> Result<()> {
    let raw = fs::read(observation).map_err(|e| e.to_string())?;
    let selection_bytes = fs::read(selection).map_err(|e| e.to_string())?;
    let selection: Selection =
        serde_json::from_slice(&selection_bytes).map_err(|e| e.to_string())?;
    if selection.format != "oddlang-bounded-json-selection-v1"
        || digest(&raw) != selection.observation_sha256
    {
        return Err("bounded JSON observation pin mismatch".into());
    }
    let value: Observation = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let expected = [
        ("byte-before-count", "capacity-unconsumed"),
        ("count-before-json", "capacity-consumed"),
        ("duplicate-member", "last-value"),
        ("empty-sequence", "empty"),
        ("malformed-json", "invalid"),
        ("malformed-after-line", "invalid"),
        ("newline-counts", "capacity"),
        ("per-record-budget", "accepted"),
        ("read-error", "invalid"),
        ("terminal-line", "invalid"),
        ("value-size-before-json", "capacity"),
    ];
    if value.format != "oddlang-bounded-json-observation-v1"
        || value.cases.len() != expected.len()
        || value.accepted_value_bytes.checked_add(1) != Some(value.rejected_value_bytes)
        || value.accepted_records.checked_add(1) != Some(value.rejected_records)
        || expected
            .iter()
            .any(|(key, result)| value.cases.get(*key).map(String::as_str) != Some(*result))
    {
        return Err("unrecognized bounded JSON native observations".into());
    }
    let graph = encode(&Contract {
        format: "oddlang-bounded-json-input-v1".into(),
        id: selection.id,
        maximum_value_bytes: value.accepted_value_bytes,
        maximum_records: value.accepted_records,
        framing: "lf-terminated".into(),
        size_boundary: "include-delimiter".into(),
        duplicate_members: "last-value".into(),
        guard_order: ["byte-limit", "record-limit", "parse"]
            .map(str::to_owned)
            .to_vec(),
        capacity_error: selection.capacity_error,
        invalid_error: selection.invalid_error,
    })?;
    let binding = encode(&selection.binding)?;
    parse(&graph, &binding)?;
    let stage = tempfile::tempdir_in(
        out.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .map_err(|e| e.to_string())?;
    let receipt = encode(&serde_json::json!({
        "format": "oddlang-bounded-json-recovery-lock-v1",
        "observationSha256": selection.observation_sha256,
        "selectionSha256": digest(&selection_bytes),
        "graphSha256": digest(&graph), "overlaySha256": digest(&binding),
        "scope": "observed guard and structural JSON cases, with additional JSON value behavior delegated to the pinned provider",
    }))?;
    for (name, bytes) in [
        ("graph.json", graph),
        ("rust.json", binding),
        ("recovery.lock", receipt),
    ] {
        fs::write(stage.path().join(name), bytes).map_err(|e| e.to_string())?;
    }
    crate::native_package::publish(stage.path(), out)
}
