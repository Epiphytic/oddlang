//! Portable ordered public records with native name and re-export bindings.
mod recover;

pub use recover::recover;

use crate::shared_declarations::unique_map;
use crate::wire_schema::{digest, encode, iri, symbol, type_symbol};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const GRAPH: &str = "oddlang-public-records-v1";
const OVERLAY: &str = "oddlang-public-records-rust-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    modules: Vec<Module>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Module {
    id: String,
    records: Vec<Record>,
    aliases: Vec<Alias>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: String,
    traits: Vec<Trait>,
    fields: Vec<Field>,
}

#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum Trait {
    Clone,
    Serialize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    id: String,
    wire: String,
    #[serde(rename = "type")]
    value: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Value {
    Bool,
    U64,
    Text,
    Octets,
}

impl Value {
    fn rust(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U64 => "u64",
            Self::Text => "::std::string::String",
            Self::Octets => "::std::vec::Vec<u8>",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    id: String,
    target: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Overlay {
    format: String,
    #[serde(deserialize_with = "unique_map")]
    identifiers: BTreeMap<String, String>,
}

impl Overlay {
    fn name(&self, identity: &str) -> &str {
        &self.identifiers[identity]
    }
}

fn claim<'a>(id: &'a str, seen: &mut BTreeSet<&'a str>) -> Result<()> {
    iri(id)?;
    if !seen.insert(id) {
        return Err(format!("duplicate public record identity: {id}"));
    }
    Ok(())
}

fn render(graph: &Graph, overlay: &Overlay, serde_available: bool) -> Result<String> {
    if graph.format != GRAPH || overlay.format != OVERLAY || graph.modules.is_empty() {
        return Err("invalid or empty public record contract".into());
    }
    let mut identities = BTreeSet::new();
    let mut module_names = BTreeSet::new();
    let mut records = BTreeMap::new();
    let mut bindings = BTreeSet::new();
    let mut bound = |id, check: fn(&str) -> bool| {
        let name = overlay
            .identifiers
            .get(id)
            .filter(|name| check(name))
            .ok_or_else(|| format!("unbound or invalid public record Rust identifier: {id}"))?;
        bindings.insert(id);
        Ok::<_, String>(name)
    };
    for module in &graph.modules {
        claim(&module.id, &mut identities)?;
        let module_name = bound(&module.id, symbol)?;
        if !module_names.insert(module_name)
            || (module.records.is_empty() && module.aliases.is_empty())
        {
            return Err(format!(
                "duplicate name or empty public record module: {module_name}"
            ));
        }
        let mut names = BTreeSet::new();
        for record in &module.records {
            claim(&record.id, &mut identities)?;
            let name = bound(&record.id, type_symbol)?;
            if !names.insert(name) || record.fields.is_empty() {
                return Err(format!("duplicate name or empty public record: {name}"));
            }
            let mut traits = BTreeSet::new();
            for capability in &record.traits {
                if !traits.insert(capability) {
                    return Err(format!("duplicate public record trait in {}", record.id));
                }
                if *capability == Trait::Serialize && !serde_available {
                    return Err("public record Serialize requires native serde with derive".into());
                }
            }
            let mut fields = BTreeSet::new();
            for field in &record.fields {
                claim(&field.id, &mut identities)?;
                if !symbol(&field.wire) || !fields.insert(&field.wire) {
                    return Err(format!(
                        "duplicate or invalid public record field: {}",
                        field.wire
                    ));
                }
            }
            records.insert(&record.id, (module_name, name));
        }
        for alias in &module.aliases {
            claim(&alias.id, &mut identities)?;
            let name = bound(&alias.id, type_symbol)?;
            if !names.insert(name) {
                return Err(format!("duplicate public record module member: {name}"));
            }
        }
    }
    if bindings.len() != overlay.identifiers.len() {
        return Err("unused public record Rust identifier binding".into());
    }
    let mut code = String::from("// Generated from the portable public record contract.\n");
    for module in &graph.modules {
        writeln!(code, "pub mod {} {{", overlay.name(&module.id)).unwrap();
        for record in &module.records {
            let traits = record
                .traits
                .iter()
                .map(|capability| match capability {
                    Trait::Clone => "::std::clone::Clone",
                    Trait::Serialize => "::serde::Serialize",
                })
                .collect::<Vec<_>>();
            if !traits.is_empty() {
                writeln!(code, "    #[derive({})]", traits.join(", ")).unwrap();
            }
            writeln!(code, "    pub struct {} {{", overlay.name(&record.id)).unwrap();
            for field in &record.fields {
                writeln!(code, "        pub {}: {},", field.wire, field.value.rust()).unwrap();
            }
            code.push_str("    }\n");
        }
        for alias in &module.aliases {
            let (owner, type_name) = records
                .get(&alias.target)
                .ok_or_else(|| format!("unresolved public record re-export: {}", alias.id))?;
            writeln!(
                code,
                "    pub use super::{owner}::{type_name} as {};",
                overlay.name(&alias.id)
            )
            .unwrap();
        }
        code.push_str("}\n");
    }
    Ok(code)
}

fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|e| e.to_string())
}

fn emit(out: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    if out.symlink_metadata().is_ok() {
        return Err("public record output already exists".into());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = io(tempfile::tempdir_in(parent))?;
    let bundle = stage.path().join("records");
    io(fs::create_dir(&bundle))?;
    for (name, bytes) in files {
        io(fs::write(bundle.join(name), bytes))?;
    }
    crate::native_package::publish(&bundle, out)
}

pub fn build(graph: &Path, overlay: &Path, out: &Path) -> Result<()> {
    build_bytes(&io(fs::read(graph))?, &io(fs::read(overlay))?, out, true)
}

pub(crate) fn build_bytes(
    graph: &[u8],
    overlay: &[u8],
    out: &Path,
    serde_available: bool,
) -> Result<()> {
    let model = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    let code = render(&model, &binding, serde_available)?;
    let lock = encode(&serde_json::json!({
        "format":"oddlang-public-record-projection-lock-v1",
        "graphSha256":digest(graph),
        "overlaySha256":digest(overlay),
        "sourceSha256":digest(code.as_bytes()),
    }))?;
    emit(
        out,
        &[("records.rs", code.as_bytes()), ("projection.lock", &lock)],
    )
}
