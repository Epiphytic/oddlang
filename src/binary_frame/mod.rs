//! Sized, discriminated binary unions and incremental one-frame decoding.
mod recover;
mod rust;
pub use recover::recover;

use crate::{
    shared_declarations::unique_map,
    wire_schema::{digest, encode, iri, json, symbol, type_symbol},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-binary-frame-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    maximum: u32,
    errors: Errors,
    #[serde(deserialize_with = "unique_map")]
    types: BTreeMap<String, External>,
    #[serde(deserialize_with = "unique_map")]
    cases: BTreeMap<String, Case>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Errors {
    header: String,
    length: String,
    limit: String,
    tag: String,
    truncated: String,
    trailing: String,
    decoder_failed: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum External {
    Enum {
        #[serde(deserialize_with = "unique_map")]
        tags: BTreeMap<String, u8>,
    },
    Fragment {},
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    tag: u8,
    form: Form,
    slots: Vec<Slot>,
    #[serde(default)]
    encode_guards: Vec<Guard>,
    #[serde(default)]
    decode_guards: Vec<Guard>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Form {
    Tuple,
    Named,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Slot {
    id: String,
    encoding: Encoding,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Encoding {
    Unsigned {
        bits: u8,
        maximum: u64,
        error: String,
    },
    Fixed {
        size: u32,
    },
    Enum {
        id: String,
        error: String,
    },
    Fragment {
        id: String,
    },
    Literal {
        value: u8,
        error: String,
    },
    Rest {
        minimum: u32,
        maximum: u32,
        error: String,
    },
}

impl Encoding {
    fn field(&self) -> bool {
        !matches!(self, Self::Literal { .. })
    }
    fn error(&self) -> Option<&str> {
        match self {
            Self::Unsigned { error, .. }
            | Self::Enum { error, .. }
            | Self::Literal { error, .. }
            | Self::Rest { error, .. } => Some(error),
            Self::Fixed { .. } | Self::Fragment { .. } => None,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    id: String,
    error: String,
    require: Predicate,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Predicate {
    Range {
        field: String,
    },
    Count {
        field: String,
    },
    AtMost {
        left: String,
        right: String,
    },
    EndAtMost {
        offset: String,
        bytes: String,
        maximum: u64,
    },
}

impl Predicate {
    fn check<'a>(&self, slot: impl Fn(&str) -> Option<&'a Encoding>) -> bool {
        match self {
            Self::Range { field } => matches!(slot(field), Some(Encoding::Unsigned { .. })),
            Self::Count { field } => matches!(slot(field), Some(Encoding::Rest { .. })),
            Self::AtMost { left, right } => matches!((slot(left),slot(right)),
                (Some(Encoding::Unsigned{bits:a,..}),Some(Encoding::Unsigned{bits:b,..})) if a==b),
            Self::EndAtMost { offset, bytes, .. } => {
                matches!(slot(offset), Some(Encoding::Unsigned { bits: 64, .. }))
                    && matches!(slot(bytes), Some(Encoding::Rest { .. }))
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    name: String,
    decoder: String,
    error_type: String,
    #[serde(deserialize_with = "unique_map")]
    errors: BTreeMap<String, Fault>,
    #[serde(deserialize_with = "unique_map")]
    cases: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    fields: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    types: BTreeMap<String, NativeType>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fault {
    variant: String,
    argument: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeType {
    path: String,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    variants: BTreeMap<String, String>,
}

fn path_ok(path: &str) -> bool {
    path.strip_prefix("crate::")
        .or_else(|| path.strip_prefix("::"))
        .is_some_and(|path| {
            path.split("::").all(symbol) && path.rsplit("::").next().is_some_and(type_symbol)
        })
}

fn declare<'a>(ids: &mut BTreeSet<&'a str>, id: &'a str) -> Result<()> {
    iri(id)?;
    if ids.insert(id) {
        Ok(())
    } else {
        Err(format!("duplicate binary frame identity: {id}"))
    }
}

impl Graph {
    fn validate(&self, binding: &Binding) -> Result<()> {
        if self.format != FORMAT
            || binding.format != "oddlang-binary-frame-rust-v1"
            || self.maximum == 0
            || self.maximum > u32::MAX - 5
            || self.cases.is_empty()
            || !type_symbol(&binding.name)
            || !type_symbol(&binding.decoder)
            || binding.name == binding.decoder
            || !path_ok(&binding.error_type)
            || !self.types.keys().eq(binding.types.keys())
            || !self.cases.keys().eq(binding.cases.keys())
        {
            return Err("invalid binary frame format or native binding".into());
        }
        iri(&self.id)?;
        let mut ids = BTreeSet::from([self.id.as_str()]);
        let mut errors = BTreeSet::from([
            self.errors.header.as_str(),
            self.errors.length.as_str(),
            self.errors.limit.as_str(),
            self.errors.tag.as_str(),
            self.errors.truncated.as_str(),
            self.errors.trailing.as_str(),
            self.errors.decoder_failed.as_str(),
        ]);
        for (id, ty) in &self.types {
            declare(&mut ids, id)?;
            let native = &binding.types[id];
            if !path_ok(&native.path) {
                return Err(format!("invalid binary frame external path: {id}"));
            }
            match ty {
                External::Enum { tags }
                    if !tags.is_empty()
                        && tags.keys().eq(native.variants.keys())
                        && tags.values().collect::<BTreeSet<_>>().len() == tags.len()
                        && native.variants.values().collect::<BTreeSet<_>>().len()
                            == native.variants.len() =>
                {
                    for id in tags.keys() {
                        declare(&mut ids, id)?;
                        if !type_symbol(&native.variants[id]) {
                            return Err(format!("invalid enum case name: {id}"));
                        }
                    }
                }
                External::Fragment {} if native.variants.is_empty() => {}
                _ => return Err(format!("invalid frame external type: {id}")),
            }
        }
        let mut tag_values = BTreeSet::new();
        let mut fields = BTreeSet::new();
        let mut case_names = BTreeSet::new();
        let mut used_types = BTreeSet::new();
        for (id, case) in &self.cases {
            declare(&mut ids, id)?;
            let name = &binding.cases[id];
            if !tag_values.insert(case.tag)
                || !type_symbol(name)
                || !case_names.insert(name)
                || case.slots.is_empty()
            {
                return Err(format!("invalid or duplicate binary frame case: {id}"));
            }
            let mut case_fields = BTreeSet::new();
            let mut minimum = 0u64;
            for (i, slot) in case.slots.iter().enumerate() {
                declare(&mut ids, &slot.id)?;
                errors.extend(slot.encoding.error());
                if slot.encoding.field() {
                    fields.insert(&slot.id);
                    if !binding.fields.get(&slot.id).is_some_and(|name| {
                        symbol(name)
                            && name.as_bytes()[0].is_ascii_lowercase()
                            && case_fields.insert(name)
                    }) {
                        return Err(format!("invalid frame field name: {}", slot.id));
                    }
                }
                let width = match &slot.encoding {
                    Encoding::Unsigned { bits, maximum, .. }
                        if matches!(bits, 8 | 16 | 32 | 64)
                            && *maximum <= u64::MAX >> (64 - bits) =>
                    {
                        (*bits / 8).into()
                    }
                    Encoding::Fixed { size } if *size != 0 => u64::from(*size),
                    Encoding::Enum { id, .. }
                        if matches!(self.types.get(id), Some(External::Enum { .. })) =>
                    {
                        used_types.insert(id);
                        1
                    }
                    Encoding::Fragment { id }
                        if matches!(self.types.get(id), Some(External::Fragment {})) =>
                    {
                        used_types.insert(id);
                        0
                    }
                    Encoding::Literal { .. } => 1,
                    Encoding::Rest {
                        minimum, maximum, ..
                    } if minimum <= maximum
                        && *maximum <= self.maximum
                        && i + 1 == case.slots.len() =>
                    {
                        u64::from(*minimum)
                    }
                    _ => return Err(format!("invalid frame slot encoding: {}", slot.id)),
                };
                minimum += width;
            }
            if minimum > self.maximum.into() || case_fields.is_empty() {
                return Err(format!("binary frame payload out of bounds: {id}"));
            }
            let find = |id: &str| {
                case.slots
                    .iter()
                    .find(|slot| slot.id == id)
                    .map(|slot| &slot.encoding)
            };
            for guard in case.encode_guards.iter().chain(&case.decode_guards) {
                declare(&mut ids, &guard.id)?;
                if !guard.require.check(find) {
                    return Err(format!("ill-typed frame guard: {}", guard.id));
                }
                errors.insert(&guard.error);
            }
            for slot in &case.slots {
                // All unbounded input fields have checked encode guards before allocation.
                let target = &slot.id;
                let required = match slot.encoding {
                    Encoding::Unsigned { bits, maximum, .. }
                        if maximum < u64::MAX >> (64 - bits) =>
                    {
                        Some(false)
                    }
                    Encoding::Rest { .. } => Some(true),
                    _ => None,
                };
                if let Some(count) = required {
                    if !case.encode_guards.iter().any(|guard| match &guard.require {
                        Predicate::Range { field } => !count && field == target,
                        Predicate::Count { field } => count && field == target,
                        Predicate::EndAtMost { offset, maximum, .. } if !count && offset == target =>
                            matches!(find(target), Some(Encoding::Unsigned {maximum: bound,..}) if maximum<=bound),
                        Predicate::AtMost { left, right } if !count && left == target =>
                            matches!((find(left),find(right)),
                                (Some(Encoding::Unsigned{maximum:left,..}),Some(Encoding::Unsigned{maximum:right,..}))
                                if right<=left) && case.encode_guards.iter().any(|guard|
                                    matches!(&guard.require,Predicate::Range{field} if field==right)),
                        _ => false,
                    }) {
                        return Err(format!("missing frame encode bound: {target}"));
                    }
                }
            }
        }
        if !fields.into_iter().eq(binding.fields.keys())
            || !used_types.into_iter().eq(self.types.keys())
            || !errors
                .iter()
                .copied()
                .eq(binding.errors.keys().map(String::as_str))
        {
            return Err("frame bindings need exact field, external type and error coverage".into());
        }
        for id in errors {
            declare(&mut ids, id)?;
            if !type_symbol(&binding.errors[id].variant) {
                return Err(format!("invalid frame error variant: {id}"));
            }
        }
        Ok(())
    }
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<()> {
    let graph: Graph = json(graph)?;
    let binding: Binding = json(binding)?;
    graph.validate(&binding)?;
    let source = rust::render(&graph, &binding);
    let witness = "independent-witness-required";
    let mut rows = vec![json!({
        "declaration":graph.id,"role":"binary-frame-header","qualification":witness,
        "expected":{"maximum":graph.maximum,"errors":graph.errors}
    })];
    for (id, case) in &graph.cases {
        rows.push(json!({"declaration":id,"role":"binary-frame-case","expected":case,"qualification":witness}));
        for (position, slot) in case.slots.iter().enumerate() {
            rows.push(
                json!({"declaration":slot.id,"role":"binary-frame-slot","qualification":witness,
                "expected":{"position":position,"encoding":slot.encoding}}),
            );
        }
        for (role, guards) in [
            ("encode-guard", &case.encode_guards),
            ("decode-guard", &case.decode_guards),
        ] {
            for (position, guard) in guards.iter().enumerate() {
                rows.push(
                    json!({"declaration":guard.id,"role":role,"qualification":witness,
                    "expected":{"position":position,"guard":guard}}),
                );
            }
        }
    }
    let obligations = encode(&json!({"domain":graph.id,"obligations":rows}))?;
    let lock = encode(&json!({
        "format":FORMAT,"graphSha256":digest(&encode(&graph)?),"bindingSha256":digest(&encode(&binding)?),
        "frameSha256":digest(source.as_bytes()),"obligationsSha256":digest(&obligations),
    }))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|err| err.to_string())?;
    for (file, bytes) in [
        ("frame.rs", source.as_bytes()),
        ("obligations.json", &obligations),
        ("projection.lock", &lock),
    ] {
        fs::write(stage.path().join(file), bytes).map_err(|err| err.to_string())?;
    }
    if output.symlink_metadata().is_ok() {
        return Err("binary frame destination already exists".into());
    }
    fs::rename(stage.path(), output).map_err(|err| err.to_string())
}
