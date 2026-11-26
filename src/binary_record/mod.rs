//! Ordered, bounded binary records with named decode and validation failures.
mod composite;
mod extension;
pub(crate) mod linkage;
use composite::{Constraint, ExternalRecord, Integrity, IntegrityBinding, RecordBinding};
mod rust;
use extension::{Guard, HashBinding, HashView, Payload, Record};

use crate::shared_declarations::unique_map;
use crate::wire_schema::{digest, encode, iri, json, symbol, type_symbol};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-binary-record-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    maximum: u32,
    layout: Vec<Slot>,
    errors: Errors,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    records: BTreeMap<String, Record>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guards: Vec<Guard>,
    #[serde(
        rename = "hashViews",
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    hash_views: BTreeMap<String, HashView>,
    #[serde(
        rename = "externalRecords",
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    external_records: BTreeMap<String, ExternalRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    constraints: Vec<Constraint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    integrity: Option<Integrity>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    fragment: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Errors {
    value: String,
    decode_size: String,
    encode_size: String,
    truncated: String,
    trailing: String,
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
    Literal {
        bytes: Vec<u8>,
        error: String,
        #[serde(
            rename = "requireFull",
            default,
            skip_serializing_if = "std::ops::Not::not"
        )]
        require_full: bool,
    },
    LengthPrefixedLiteral {
        bytes: Vec<u8>,
        maximum: u32,
        overflow: String,
        error: String,
    },
    UnsignedBe {
        bits: u8,
        minimum: u64,
        maximum: u64,
        #[serde(rename = "readError", default, skip_serializing_if = "Option::is_none")]
        read_error: Option<String>,
        #[serde(
            rename = "valueError",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        value_error: Option<String>,
    },
    Bool {
        error: String,
    },
    SignedBe {
        bits: u8,
    },
    Inline {
        id: String,
    },
    Nested {
        record: String,
        maximum: u32,
        overflow: String,
    },
    Optional {
        payload: Payload,
        error: String,
    },
    FixedBytes {
        size: u32,
    },
    Bytes {
        minimum: u32,
        maximum: u32,
        overflow: String,
        #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
        excludes: BTreeSet<u8>,
        #[serde(
            rename = "valueError",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        value_error: Option<String>,
    },
    AsciiAtom {
        minimum: u32,
        maximum: u32,
        punctuation: BTreeSet<u8>,
        overflow: String,
        error: String,
    },
    EnumU8 {
        external: String,
        #[serde(deserialize_with = "unique_map")]
        tags: BTreeMap<String, u8>,
        error: String,
    },
}

impl Encoding {
    fn field(&self) -> bool {
        !matches!(
            self,
            Self::Literal { .. } | Self::LengthPrefixedLiteral { .. }
        )
    }

    fn errors(&self) -> Vec<&str> {
        match self {
            Self::Literal { error, .. }
            | Self::EnumU8 { error, .. }
            | Self::Bool { error }
            | Self::Optional { error, .. } => vec![error],
            Self::LengthPrefixedLiteral {
                overflow, error, ..
            }
            | Self::AsciiAtom {
                overflow, error, ..
            } => vec![overflow, error],
            Self::Nested { overflow, .. } => vec![overflow],
            Self::SignedBe { .. } | Self::Inline { .. } => Vec::new(),
            Self::Bytes {
                overflow,
                value_error,
                ..
            } => std::iter::once(overflow.as_str())
                .chain(value_error.as_deref())
                .collect(),
            Self::UnsignedBe {
                read_error,
                value_error,
                ..
            } => read_error
                .as_deref()
                .into_iter()
                .chain(value_error.as_deref())
                .collect(),
            Self::FixedBytes { .. } => Vec::new(),
        }
    }

    fn width(&self, records: &BTreeMap<String, Record>) -> Result<(u64, u64)> {
        let fixed = |size| Ok((size, size));
        let framed = |minimum: u32, maximum: u32| {
            if minimum > maximum {
                return Err("invalid binary byte length interval".into());
            }
            Ok((4 + u64::from(minimum), 4 + u64::from(maximum)))
        };
        match self {
            Self::UnsignedBe {
                bits,
                minimum,
                maximum,
                ..
            } if matches!(bits, 8 | 16 | 32 | 64)
                && minimum <= maximum
                && *maximum <= u64::MAX >> (64 - bits) =>
            {
                fixed((*bits / 8).into())
            }
            Self::UnsignedBe { .. } => Err("invalid unsigned width or range".into()),
            Self::SignedBe { bits } if matches!(bits, 8 | 16 | 32 | 64) => {
                fixed((*bits / 8).into())
            }
            Self::SignedBe { .. } => Err("invalid signed integer width".into()),
            Self::Inline { id } => fixed(Payload::Record { id: id.clone() }.width(records)?),
            Self::Nested { maximum, .. } => framed(0, *maximum),
            Self::FixedBytes { size } if *size != 0 => fixed((*size).into()),
            Self::FixedBytes { .. } => Err("empty fixed byte array".into()),
            Self::Literal { bytes, .. } if !bytes.is_empty() => fixed(bytes.len() as u64),
            Self::LengthPrefixedLiteral { bytes, maximum, .. }
                if bytes.len() <= *maximum as usize =>
            {
                fixed(4 + bytes.len() as u64)
            }
            Self::Literal { .. } | Self::LengthPrefixedLiteral { .. } => {
                Err("invalid binary literal width".into())
            }
            Self::Bytes {
                minimum, maximum, ..
            } => framed(*minimum, *maximum),
            Self::AsciiAtom {
                minimum,
                maximum,
                punctuation,
                ..
            } => {
                if !punctuation.iter().all(|byte| byte.is_ascii_punctuation()) {
                    return Err("invalid ASCII punctuation policy".into());
                }
                framed(*minimum, *maximum)
            }
            Self::EnumU8 { .. } | Self::Bool { .. } => fixed(1),
            Self::Optional { payload, .. } => Ok((1, 1 + payload.width(records)?)),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    name: String,
    #[serde(deserialize_with = "unique_map")]
    names: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    externals: BTreeMap<String, External>,
    error_type: String,
    #[serde(deserialize_with = "unique_map")]
    errors: BTreeMap<String, Fault>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    hash_views: BTreeMap<String, HashBinding>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    external_records: BTreeMap<String, RecordBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    integrity: Option<IntegrityBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    existing_type: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct External {
    path: String,
    #[serde(deserialize_with = "unique_map")]
    variants: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fault {
    variant: String,
    argument: Option<String>,
}

fn absolute_path(path: &str) -> bool {
    let path = path
        .strip_prefix("crate::")
        .or_else(|| path.strip_prefix("::"));
    path.is_some_and(|path| {
        path.split("::").all(symbol) && path.rsplit("::").next().is_some_and(type_symbol)
    })
}

fn unique<T: Ord>(items: impl IntoIterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    items.into_iter().all(|item| seen.insert(item))
}

impl Graph {
    fn validate(&self, binding: &Binding) -> Result<()> {
        if self.format != FORMAT
            || binding.format != "oddlang-binary-record-rust-v1"
            || !type_symbol(&binding.name)
            || !absolute_path(&binding.error_type)
            || self.layout.is_empty()
        {
            return Err("invalid binary record format, layout, or native declaration".into());
        }
        if (self.fragment && self.integrity.is_some())
            || binding.existing_type.as_deref().is_some_and(|path| {
                !self.fragment
                    || !absolute_path(path)
                    || !self.hash_views.is_empty()
                    || !self.records.is_empty()
            })
        {
            return Err(
                "prefix fragments require an unsealed layout and a qualified existing type".into(),
            );
        }
        iri(&self.id)?;
        let mut ids = BTreeSet::from([self.id.as_str()]);
        let mut names = BTreeSet::new();
        extension::validate_records(self, binding, &mut ids, &mut names)?;
        let mut fields = BTreeSet::new();
        let mut externals = BTreeMap::new();
        let mut errors = BTreeSet::from([
            self.errors.value.as_str(),
            self.errors.decode_size.as_str(),
            self.errors.encode_size.as_str(),
            self.errors.truncated.as_str(),
            self.errors.trailing.as_str(),
        ]);
        let (mut minimum, mut maximum) = (0u64, 0u64);
        if self.integrity.is_some() {
            minimum += 32;
            maximum += 32;
        }
        for slot in &self.layout {
            iri(&slot.id)?;
            if !ids.insert(&slot.id) {
                return Err(format!("duplicate binary layout identity: {}", slot.id));
            }
            if slot.encoding.field() {
                names.insert(&slot.id);
                fields.insert(&slot.id);
            }
            let (min, max) = slot
                .encoding
                .width(&self.records)
                .map_err(|err| format!("{}: {err}", slot.id))?;
            minimum += min;
            maximum += max;
            if maximum > u64::from(u32::MAX) {
                return Err("binary layout exceeds 32-bit length accounting".into());
            }
            errors.extend(slot.encoding.errors());
            if let Encoding::EnumU8 { external, tags, .. } = &slot.encoding {
                if tags.is_empty()
                    || !unique(tags.values())
                    || externals
                        .insert(external, tags)
                        .is_some_and(|prior| prior != tags)
                {
                    return Err(format!("invalid enum tag table: {external}"));
                }
            }
        }
        if minimum > self.maximum.into() || fields.is_empty() {
            return Err("binary record cannot fit its maximum or has no fields".into());
        }
        if !names.into_iter().eq(binding.names.keys())
            || !unique(fields.iter().filter_map(|id| binding.names.get(*id)))
            || fields.iter().any(|id| {
                !binding
                    .names
                    .get(*id)
                    .is_some_and(|name| symbol(name) && name.as_bytes()[0].is_ascii_lowercase())
            })
        {
            return Err("missing, invalid or duplicate binary field name".into());
        }
        if !externals.keys().copied().eq(binding.externals.keys()) {
            return Err("binary external bindings must match the declared enum types".into());
        }
        for (external, tags) in externals {
            iri(external)?;
            let native = &binding.externals[external];
            if !ids.insert(external)
                || !absolute_path(&native.path)
                || !tags.keys().eq(native.variants.keys())
                || !unique(native.variants.values())
                || native.variants.values().any(|name| !type_symbol(name))
            {
                return Err(format!("invalid enum binding: {external}"));
            }
            for variant in tags.keys() {
                iri(variant)?;
                if !ids.insert(variant) {
                    return Err(format!("duplicate enum variant identity: {variant}"));
                }
            }
        }
        extension::validate_checks(self, binding, &mut ids, &mut errors)?;
        composite::validate(self, binding, &mut ids, &mut errors)?;
        if !errors
            .iter()
            .copied()
            .eq(binding.errors.keys().map(String::as_str))
        {
            return Err("binary error bindings must cover exactly the named failures".into());
        }
        for error in errors {
            iri(error)?;
            if !ids.insert(error) {
                return Err(format!("binary error aliases a declaration: {error}"));
            }
            if !type_symbol(&binding.errors[error].variant) {
                return Err(format!("invalid native binary error variant: {error}"));
            }
        }
        Ok(())
    }

    fn obligations(&self) -> serde_json::Value {
        let witness = "independent-witness-required";
        let mut obligations = vec![json!({
            "declaration": self.id, "role": "binary-record",
            "expected": {"maximumBytes": self.maximum, "errors": self.errors},
            "qualification": witness,
        })];
        for (index, slot) in self.layout.iter().enumerate() {
            obligations.push(json!({
                "declaration": slot.id, "role": "binary-slot",
                "expected": {"position": index, "encoding": slot.encoding},
                "qualification": witness,
            }));
        }
        for (id, record) in &self.records {
            obligations.push(json!({"declaration":id,"role":"inline-binary-record","qualification":witness,"expected":record}));
        }
        for guard in &self.guards {
            obligations.push(json!({"declaration":guard.id,"role":"binary-guard","qualification":witness,"expected":guard}));
        }
        for (id, view) in &self.hash_views {
            obligations.push(json!({"declaration":id,"role":"typed-hash-view","qualification":witness,"expected":view}));
        }
        for constraint in &self.constraints {
            obligations.push(json!({"declaration":constraint.id,"role":"composite-binary-constraint","qualification":witness,"expected":constraint}));
        }
        if let Some(integrity) = &self.integrity {
            obligations.push(json!({"declaration":self.id,"role":"binary-integrity","qualification":witness,"expected":integrity}));
        }
        json!({"domain": self.id, "obligations": obligations})
    }
}

pub fn build(graph: &Path, binding: &Path, output: &Path) -> Result<()> {
    build_record(graph, binding, output, false)
}

pub(crate) fn build_bound_fragment(graph: &Path, binding: &Path, output: &Path) -> Result<()> {
    build_record(graph, binding, output, true)
}

fn build_record(graph: &Path, binding: &Path, output: &Path, require_bound: bool) -> Result<()> {
    let graph: Graph = json(graph)?;
    let binding: Binding = json(binding)?;
    graph.validate(&binding)?;
    if require_bound && (!graph.fragment || binding.existing_type.is_none()) {
        return Err("binary fragment components must bind an existing type".into());
    }
    let source = rust::render(&graph, &binding);
    let obligations = encode(&graph.obligations())?;
    let lock = encode(&json!({
        "format": FORMAT,
        "graphSha256": digest(&encode(&graph)?),
        "bindingSha256": digest(&encode(&binding)?),
        "recordSha256": digest(source.as_bytes()),
        "obligationsSha256": digest(&obligations),
    }))?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, bytes) in [
        ("record.rs", source.as_bytes()),
        ("obligations.json", &obligations),
        ("projection.lock", &lock),
    ] {
        fs::write(stage.path().join(name), bytes).map_err(|e| e.to_string())?;
    }
    if output.symlink_metadata().is_ok() {
        return Err("binary record destination already exists".into());
    }
    fs::rename(stage.path(), output).map_err(|e| e.to_string())
}
