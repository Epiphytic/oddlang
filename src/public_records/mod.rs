//! Portable ordered public records with native name and re-export bindings.
mod recover;
mod root;

pub use recover::recover;
pub use root::recover_root;

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
    #[serde(rename = "readPolicy", skip_serializing_if = "Option::is_none")]
    read_policy: Option<ReadPolicy>,
    fields: Vec<Field>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadPolicy {
    shape: InputShape,
    unknown_fields: UnknownFields,
}

impl ReadPolicy {
    fn rejects_unknown(&self) -> bool {
        match self.shape {
            InputShape::ObjectOrOrderedSequence => self.unknown_fields == UnknownFields::Reject,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum InputShape {
    ObjectOrOrderedSequence,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum UnknownFields {
    Reject,
    Ignore,
}

#[derive(Deserialize, Serialize, Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum Trait {
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Debug,
    Default,
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
    U32,
    U64,
    Text,
    Octets,
    Path,
    Optional(Box<Value>),
    External(String),
}

impl Value {
    fn rust(&self, overlay: &Overlay) -> String {
        match self {
            Self::Bool => "bool".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::Text => "::std::string::String".into(),
            Self::Octets => "::std::vec::Vec<u8>".into(),
            Self::Path => "::std::path::PathBuf".into(),
            Self::Optional(inner) => format!("::core::option::Option<{}>", inner.rust(overlay)),
            Self::External(id) => overlay.external_types[id].join("::"),
        }
    }

    fn contained(&self) -> Result<&Value> {
        match self {
            Self::Optional(inner) if matches!(**inner, Self::Optional(_)) => {
                Err("nested optional public record values have no distinct wire shape".into())
            }
            Self::Optional(inner) => Ok(inner),
            _ => Ok(self),
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
    #[serde(
        rename = "flattenModule",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    flatten_module: bool,
    #[serde(deserialize_with = "unique_map")]
    identifiers: BTreeMap<String, String>,
    #[serde(
        rename = "externalTypes",
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    external_types: BTreeMap<String, Vec<String>>,
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

fn validate_external_types(graph: &Graph, overlay: &Overlay) -> Result<()> {
    let mut consumed = BTreeSet::new();
    for record in graph.modules.iter().flat_map(|module| &module.records) {
        for field in &record.fields {
            if let Value::External(id) = field.value.contained()? {
                iri(id)?;
                if matches!(field.value, Value::External(_))
                    && record.traits.contains(&Trait::Default)
                {
                    return Err("an external public record choice has no declared default".into());
                }
                consumed.insert(id);
            }
        }
    }
    if consumed.into_iter().ne(overlay.external_types.keys()) {
        return Err(
            "external public record choice bindings must match the declared identities".into(),
        );
    }
    for path in overlay.external_types.values() {
        if path.len() < 2
            || path[0] != "crate"
            || !path[1..path.len() - 1].iter().all(|name| symbol(name))
            || !type_symbol(path.last().expect("verified nonempty type path"))
        {
            return Err("invalid external public record choice path".into());
        }
    }
    Ok(())
}

pub(crate) fn external_choice_links(
    graph: &[u8],
    binding: &[u8],
) -> Result<BTreeMap<String, Vec<String>>> {
    let graph = serde_json::from_slice(graph).map_err(|e| e.to_string())?;
    let binding = serde_json::from_slice(binding).map_err(|e| e.to_string())?;
    validate_external_types(&graph, &binding)?;
    Ok(binding.external_types)
}

fn render(graph: &Graph, overlay: &Overlay, serde_available: bool) -> Result<String> {
    if graph.format != GRAPH || overlay.format != OVERLAY || graph.modules.is_empty() {
        return Err("invalid or empty public record contract".into());
    }
    if overlay.flatten_module && (graph.modules.len() != 1 || !graph.modules[0].aliases.is_empty())
    {
        return Err(
            "a flat public record module requires exactly one module and no aliases".into(),
        );
    }
    validate_external_types(graph, overlay)?;
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
                if matches!(capability, Trait::Serialize | Trait::Deserialize) && !serde_available {
                    return Err(format!(
                        "public record {capability:?} requires native serde with derive"
                    ));
                }
            }
            if record.read_policy.is_some() && !traits.contains(&Trait::Deserialize) {
                return Err("public record read policy requires Deserialize".into());
            }
            if traits.contains(&Trait::Eq) && !traits.contains(&Trait::PartialEq) {
                return Err("public record Eq requires PartialEq".into());
            }
            if (traits.contains(&Trait::Serialize) || traits.contains(&Trait::Deserialize))
                && record
                    .fields
                    .iter()
                    .any(|field| matches!(field.value.contained(), Ok(Value::Path)))
            {
                return Err("serialized public path field has no portable wire rule".into());
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
    if overlay
        .external_types
        .keys()
        .any(|id| identities.contains(id.as_str()))
    {
        return Err("external public record choice collides with a record declaration".into());
    }
    if bindings.len() != overlay.identifiers.len() {
        return Err("unused public record Rust identifier binding".into());
    }
    let mut code = String::from("// Generated from the portable public record contract.\n");
    let indent = if overlay.flatten_module { "" } else { "    " };
    for module in &graph.modules {
        if !overlay.flatten_module {
            writeln!(code, "pub mod {} {{", overlay.name(&module.id)).unwrap();
        }
        for record in &module.records {
            let traits = record
                .traits
                .iter()
                .map(|capability| match capability {
                    Trait::Clone => "::std::clone::Clone",
                    Trait::Serialize => "::serde::Serialize",
                    Trait::Deserialize => "::serde::Deserialize",
                    Trait::PartialEq => "::core::cmp::PartialEq",
                    Trait::Eq => "::core::cmp::Eq",
                    Trait::Debug => "::core::fmt::Debug",
                    Trait::Default => "::core::default::Default",
                })
                .collect::<Vec<_>>();
            if !traits.is_empty() {
                writeln!(code, "{indent}#[derive({})]", traits.join(", ")).unwrap();
            }
            if record.traits.contains(&Trait::Deserialize)
                && record
                    .read_policy
                    .as_ref()
                    .is_none_or(ReadPolicy::rejects_unknown)
            {
                writeln!(code, "{indent}#[serde(deny_unknown_fields)]").unwrap();
            }
            writeln!(code, "{indent}pub struct {} {{", overlay.name(&record.id)).unwrap();
            for field in &record.fields {
                writeln!(
                    code,
                    "{indent}    pub {}: {},",
                    field.wire,
                    field.value.rust(overlay)
                )
                .unwrap();
            }
            writeln!(code, "{indent}}}").unwrap();
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
        if !overlay.flatten_module {
            code.push_str("}\n");
        }
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
