//! Ordered JSON record and adjacent-union schemas with typed validation guards.
use oxigraph::model::NamedNode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod hash_view;
pub(crate) use hash_view::{sha256_begin, sha256_enum, sha256_hex};
mod obligations;
pub mod protocol;
pub mod recover;
pub mod record_writer;
mod render;

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-json-wire-schema-v1";
const BINDING_FORMAT: &str = "oddlang-json-wire-schema-rust-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Schema {
    format: String,
    id: String,
    types: Vec<Definition>,
    guards: Vec<Guard>,
    #[serde(rename = "hashViews", default, skip_serializing_if = "Vec::is_empty")]
    hash_views: Vec<hash_view::View>,
    #[serde(
        rename = "hashTags",
        default,
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    hash_tags: BTreeMap<String, BTreeMap<String, u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unresolved: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Definition {
    Record {
        id: String,
        fields: Vec<Field>,
    },
    Adjacent {
        id: String,
        tag: String,
        content: String,
        variants: Vec<Variant>,
    },
    External {
        id: String,
    },
}

impl Definition {
    fn id(&self) -> &str {
        match self {
            Self::Record { id, .. } | Self::Adjacent { id, .. } | Self::External { id } => id,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    #[serde(
        rename = "requirePresence",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    require_presence: bool,
    id: String,
    name: String,
    #[serde(rename = "type")]
    shape: Shape,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Shape {
    Bool {},
    Text {},
    Unsigned { bits: u8 },
    Optional { item: Box<Shape> },
    List { item: Box<Shape> },
    Ref { id: String },
    LowerHex { bytes: usize, diagnostic: String },
    BoundedBase64 { policy: BinaryScalar },
    BoundedBase64List { policy: BinaryList },
    Unresolved { reason: String },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BinaryScalar {
    max_bytes: u32,
    encoded_limit_message: String,
    decoded_limit_message: String,
    syntax_message: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BinaryList {
    max_items: u32,
    max_item_bytes: u32,
    max_total_bytes: u32,
    per_item_overhead: u32,
    list_limit_message: String,
    encoded_limit_message: String,
    decoded_limit_message: String,
    syntax_message: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    id: String,
    name: String,
    payload: Payload,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Payload {
    Unit {},
    Newtype {
        #[serde(rename = "type")]
        shape: Shape,
    },
    Record {
        fields: Vec<Field>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    id: String,
    owner: String,
    variant: Option<String>,
    when: Option<Predicate>,
    require: Predicate,
    error: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Predicate {
    Bool {
        field: String,
        value: bool,
    },
    Present {
        field: String,
    },
    UnsignedRange {
        field: String,
        minimum: u64,
        maximum: u64,
    },
    Count {
        field: String,
        minimum: u32,
        maximum: u32,
    },
    Unique {
        field: String,
    },
    ExcludesByte {
        field: String,
        byte: u8,
    },
    AtMost {
        left: String,
        right: String,
    },
}

impl Predicate {
    fn validate(&self, fields: &[Field]) -> Result<()> {
        let find = |id: &str| {
            fields
                .iter()
                .find(|field| field.id == id)
                .map(|field| &field.shape)
                .ok_or_else(|| format!("unknown predicate field {id}"))
        };
        let allowed = match self {
            Self::Bool { field, .. } => matches!(find(field)?, Shape::Bool {}),
            Self::Present { field } => matches!(find(field)?, Shape::Optional { .. }),
            Self::UnsignedRange {
                field,
                minimum,
                maximum,
            } => match find(field)? {
                Shape::Unsigned { bits } => {
                    minimum <= maximum && *maximum <= u64::MAX >> (64 - *bits)
                }
                _ => false,
            },
            Self::Count {
                field,
                minimum,
                maximum,
            } => {
                minimum <= maximum
                    && matches!(
                        find(field)?,
                        Shape::List { .. }
                            | Shape::BoundedBase64List { .. }
                            | Shape::BoundedBase64 { .. }
                    )
            }
            Self::ExcludesByte { field, .. } => matches!(
                find(field)?,
                Shape::BoundedBase64 { .. } | Shape::LowerHex { .. }
            ),
            Self::Unique { field } => match find(field)? {
                Shape::List { item } => matches!(
                    item.as_ref(),
                    Shape::Bool {} | Shape::Text {} | Shape::Unsigned { .. }
                ),
                _ => false,
            },
            Self::AtMost { left, right } => matches!(
                (find(left)?, find(right)?),
                (Shape::Unsigned { .. }, Shape::Unsigned { .. })
            ),
        };
        if allowed {
            Ok(())
        } else {
            Err("predicate field type or bound is invalid".into())
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    format: String,
    names: BTreeMap<String, String>,
    derives: BTreeMap<String, BTreeSet<String>>,
    #[serde(
        rename = "nativeValidation",
        default,
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    native_validation: BTreeMap<String, NativeValidation>,
    externals: BTreeMap<String, String>,
    #[serde(
        rename = "hashViews",
        default,
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    hash_views: BTreeMap<String, hash_view::ViewBinding>,
    #[serde(
        rename = "hashVariants",
        default,
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    hash_variants: BTreeMap<String, String>,
    #[serde(
        rename = "externalDerives",
        default,
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    external_derives: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeValidation {
    method: String,
    type_path: String,
    errors: BTreeMap<String, Fault>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fault {
    variant: String,
    argument: Option<String>,
}
pub(crate) fn symbol(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name != "gen"
        && syn::parse_str::<syn::Ident>(name).is_ok()
}

pub(crate) fn type_symbol(name: &str) -> bool {
    symbol(name) && name.as_bytes()[0].is_ascii_uppercase()
}

fn native_path(path: &str) -> bool {
    let mut parts = path
        .strip_prefix("::")
        .unwrap_or(path)
        .split("::")
        .peekable();
    let mut prefix = true;
    while let Some(part) = parts.next() {
        if prefix && matches!(part, "crate" | "self" | "super") && parts.peek().is_some() {
            continue;
        }
        prefix = false;
        if !symbol(part) {
            return false;
        }
    }
    !prefix
}

pub(crate) fn iri(value: &str) -> Result<()> {
    NamedNode::new(value)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn distinct<T: Ord>(values: impl IntoIterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    values.into_iter().all(|value| seen.insert(value))
}

impl Schema {
    fn validate(&self, binding: &Binding) -> Result<()> {
        if self.format != FORMAT || binding.format != BINDING_FORMAT {
            return Err("unsupported wire schema or Rust binding".into());
        }
        iri(&self.id)?;
        if let Some(gap) = self.unresolved.first() {
            return Err(format!("unresolved structural evidence: {gap}"));
        }
        let declared: BTreeMap<_, _> = self.types.iter().map(|ty| (ty.id(), ty)).collect();
        if declared.len() != self.types.len() || declared.is_empty() {
            return Err("wire schema requires distinct named definitions".into());
        }
        let mut ids = BTreeSet::new();
        let mut externals = BTreeSet::new();
        let mut native_types = BTreeSet::new();
        for ty in &self.types {
            iri(ty.id())?;
            if !ids.insert(ty.id()) {
                return Err("duplicate wire schema identity".into());
            }
            if matches!(ty, Definition::External { .. }) {
                let path = binding
                    .externals
                    .get(ty.id())
                    .ok_or("missing native external reference")?;
                if !native_path(path) {
                    return Err("invalid native external path".into());
                }
                externals.insert(ty.id());
                continue;
            }
            if !binding
                .names
                .get(ty.id())
                .is_some_and(|name| type_symbol(name) && native_types.insert(name))
            {
                return Err("invalid or duplicate native type binding".into());
            }
            if let Some(traits) = binding.derives.get(ty.id()) {
                if !allowed_derives(traits) {
                    return Err("unsupported or incomplete Rust derive binding".into());
                }
            }
            match ty {
                Definition::Record { fields, .. } => {
                    check_fields(fields, binding, &mut ids, &declared)?
                }
                Definition::Adjacent {
                    tag,
                    content,
                    variants,
                    ..
                } => {
                    if tag.is_empty()
                        || content.is_empty()
                        || tag == content
                        || variants.is_empty()
                        || !distinct(variants.iter().map(|variant| &variant.name))
                    {
                        return Err(
                            "adjacent union requires distinct tag, content and case names".into(),
                        );
                    }
                    let mut names = BTreeSet::new();
                    for variant in variants {
                        iri(&variant.id)?;
                        if !ids.insert(&variant.id)
                            || variant.name.is_empty()
                            || !binding
                                .names
                                .get(&variant.id)
                                .is_some_and(|name| type_symbol(name) && names.insert(name))
                        {
                            return Err("invalid wire variant or Rust binding".into());
                        }
                        match &variant.payload {
                            Payload::Unit {} => {}
                            Payload::Record { fields } => {
                                check_fields(fields, binding, &mut ids, &declared)?
                            }
                            Payload::Newtype { shape } => {
                                check_shape(shape, &declared)?;
                                if matches!(
                                    shape,
                                    Shape::LowerHex { .. }
                                        | Shape::BoundedBase64List { .. }
                                        | Shape::BoundedBase64 { .. }
                                ) {
                                    return Err("wire adapter requires a named record field".into());
                                }
                            }
                        }
                    }
                }
                Definition::External { .. } => unreachable!("externals are handled first"),
            }
        }
        if !ids
            .difference(&externals)
            .copied()
            .eq(binding.names.keys().map(String::as_str))
            || externals != binding.externals.keys().map(String::as_str).collect()
            || binding
                .derives
                .keys()
                .any(|id| !declared.contains_key(id.as_str()) || externals.contains(id.as_str()))
        {
            return Err("Rust binding must cover exactly the declared identities".into());
        }
        for (id, traits) in &binding.external_derives {
            if !externals.contains(id.as_str()) || !allowed_derives(traits) {
                return Err("invalid external derive declaration".into());
            }
        }
        for definition in &self.types {
            if let Some(traits) = binding.derives.get(definition.id()) {
                let fields = definition_shapes(definition);
                for trait_name in traits {
                    if fields
                        .iter()
                        .any(|shape| !supports_derive(shape, trait_name, binding))
                    {
                        return Err(format!(
                            "{}: referenced field does not support derive {trait_name}",
                            definition.id()
                        ));
                    }
                }
            }
        }
        check_cycles(&declared)?;
        let mut guard_ids = BTreeSet::new();
        for guard in &self.guards {
            iri(&guard.id)?;
            iri(&guard.error)?;
            if !guard_ids.insert(&guard.id) {
                return Err("duplicate guard identity".into());
            }
            let fields = match (declared.get(guard.owner.as_str()), guard.variant.as_deref()) {
                (Some(Definition::Record { fields, .. }), None) => fields,
                (Some(Definition::Adjacent { variants, .. }), Some(id)) => {
                    match variants.iter().find(|variant| variant.id == id) {
                        Some(Variant {
                            payload: Payload::Record { fields },
                            ..
                        }) => fields,
                        _ => return Err("guard requires a named record payload".into()),
                    }
                }
                _ => return Err("invalid guard owner or variant".into()),
            };
            if let Some(when) = &guard.when {
                when.validate(fields)?;
            }
            guard.require.validate(fields)?;
        }
        for (owner, native) in &binding.native_validation {
            let declared_errors: BTreeSet<_> = self
                .guards
                .iter()
                .filter(|g| &g.owner == owner)
                .map(|g| &g.error)
                .collect();
            if declared_errors.is_empty()
                || !native.errors.keys().eq(declared_errors)
                || !symbol(&native.method)
                || !native_path(&native.type_path)
                || !native.type_path.split("::").last().is_some_and(type_symbol)
                || native.errors.values().any(|e| !symbol(&e.variant))
            {
                return Err(format!("invalid native validation binding: {owner}"));
            }
        }
        hash_view::validate(self, binding, &declared, &ids)
    }
}

fn check_fields<'a>(
    fields: &'a [Field],
    binding: &Binding,
    ids: &mut BTreeSet<&'a str>,
    declared: &BTreeMap<&str, &Definition>,
) -> Result<()> {
    let mut names = BTreeSet::new();
    if !distinct(fields.iter().map(|field| &field.name)) {
        return Err("duplicate record wire field".into());
    }
    for field in fields {
        iri(&field.id)?;
        if !ids.insert(&field.id)
            || field.name.is_empty()
            || !binding
                .names
                .get(&field.id)
                .is_some_and(|name| symbol(name) && names.insert(name))
        {
            return Err("invalid record field or Rust binding".into());
        }
        if field.require_presence && !matches!(field.shape, Shape::Optional { .. }) {
            return Err("requirePresence applies to a nullable field".into());
        }
        check_shape(&field.shape, declared)?;
    }
    Ok(())
}

fn check_shape(shape: &Shape, declared: &BTreeMap<&str, &Definition>) -> Result<()> {
    match shape {
        Shape::Bool {} | Shape::Text {} => Ok(()),
        Shape::Unsigned { bits } if matches!(bits, 8 | 16 | 32 | 64) => Ok(()),
        Shape::Optional { item } | Shape::List { item } => {
            if matches!(
                item.as_ref(),
                Shape::LowerHex { .. }
                    | Shape::BoundedBase64List { .. }
                    | Shape::BoundedBase64 { .. }
            ) {
                return Err("wire adapter requires a named record field".into());
            }
            check_shape(item, declared)
        }
        Shape::Ref { id } if declared.contains_key(id.as_str()) => Ok(()),
        Shape::LowerHex { bytes, .. } if (1..=4096).contains(bytes) => Ok(()),
        Shape::BoundedBase64 { policy } if policy.max_bytes.div_ceil(3) <= u32::MAX / 4 => Ok(()),
        Shape::BoundedBase64List { policy }
            if policy.max_item_bytes <= policy.max_total_bytes
                && policy.max_item_bytes.div_ceil(3) <= i32::MAX as u32 / 4 =>
        {
            Ok(())
        }
        Shape::Unresolved { reason } => Err(format!("unresolved shape: {reason}")),
        _ => Err("unknown wire type or unsupported width".into()),
    }
}

fn allowed_derives(traits: &BTreeSet<String>) -> bool {
    traits.iter().all(|name| {
        matches!(
            name.as_str(),
            "Clone" | "Copy" | "Debug" | "Eq" | "PartialEq"
        )
    }) && (!traits.contains("Copy") || traits.contains("Clone"))
        && (!traits.contains("Eq") || traits.contains("PartialEq"))
}

fn definition_shapes(definition: &Definition) -> Vec<&Shape> {
    let mut shapes = Vec::new();
    match definition {
        Definition::Record { fields, .. } => shapes.extend(fields.iter().map(|f| &f.shape)),
        Definition::Adjacent { variants, .. } => {
            for variant in variants {
                match &variant.payload {
                    Payload::Record { fields } => shapes.extend(fields.iter().map(|f| &f.shape)),
                    Payload::Newtype { shape } => shapes.push(shape),
                    Payload::Unit {} => {}
                }
            }
        }
        Definition::External { .. } => {}
    }
    shapes
}

fn supports_derive(shape: &Shape, trait_name: &str, binding: &Binding) -> bool {
    match shape {
        Shape::Ref { id } => binding
            .derives
            .get(id)
            .or_else(|| binding.external_derives.get(id))
            .is_some_and(|traits| traits.contains(trait_name)),
        Shape::Optional { item } => supports_derive(item, trait_name, binding),
        Shape::List { item } => trait_name != "Copy" && supports_derive(item, trait_name, binding),
        Shape::Text {} | Shape::BoundedBase64List { .. } | Shape::BoundedBase64 { .. } => {
            trait_name != "Copy"
        }
        _ => true,
    }
}

fn check_cycles(declared: &BTreeMap<&str, &Definition>) -> Result<()> {
    fn reference(shape: &Shape) -> Option<&str> {
        match shape {
            Shape::Ref { id } => Some(id),
            Shape::Optional { item } | Shape::List { item } => reference(item),
            _ => None,
        }
    }

    fn visit<'a>(
        id: &'a str,
        declared: &BTreeMap<&'a str, &'a Definition>,
        active: &mut BTreeSet<&'a str>,
        finished: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if finished.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            return Err(format!("recursive wire schema reference at {id}"));
        }
        let mut refs = Vec::new();
        match declared[id] {
            Definition::Record { fields, .. } => {
                refs.extend(fields.iter().filter_map(|field| reference(&field.shape)))
            }
            Definition::Adjacent { variants, .. } => {
                for variant in variants {
                    match &variant.payload {
                        Payload::Record { fields } => {
                            refs.extend(fields.iter().filter_map(|field| reference(&field.shape)))
                        }
                        Payload::Newtype { shape } => refs.extend(reference(shape)),
                        Payload::Unit {} => {}
                    }
                }
            }
            Definition::External { .. } => {}
        }
        for other in refs {
            visit(other, declared, active, finished)?;
        }
        active.remove(id);
        finished.insert(id);
        Ok(())
    }

    let mut active = BTreeSet::new();
    let mut finished = BTreeSet::new();
    for id in declared.keys() {
        visit(id, declared, &mut active, &mut finished)?;
    }
    Ok(())
}

fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|error| error.to_string())
}

pub(crate) fn json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&io(fs::read(path))?)
        .map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|error| error.to_string())
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn publish(output: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = io(tempfile::tempdir_in(parent))?;
    for (name, bytes) in files {
        io(fs::write(stage.path().join(name), bytes))?;
    }
    if output.symlink_metadata().is_ok() {
        return Err("wire schema destination already exists".into());
    }
    io(fs::rename(stage.path(), output))
}

pub fn build(graph: &Path, binding: &Path, destination: &Path) -> Result<()> {
    let mut graph: Schema = json(graph)?;
    graph.types.sort_by(|left, right| left.id().cmp(right.id()));
    graph
        .hash_views
        .sort_by(|left, right| left.id.cmp(&right.id));
    let binding: Binding = json(binding)?;
    graph.validate(&binding)?;
    let source = render::project(&graph, &binding);
    let obligations = encode(&obligations::derive(&graph))?;
    let lock = serde_json::json!({
        "format": FORMAT,
        "graphSha256": digest(&encode(&graph)?),
        "bindingSha256": digest(&encode(&binding)?),
        "wireSha256": digest(source.as_bytes()),
        "obligationsSha256": digest(&obligations),
    });
    publish(
        destination,
        &[
            ("wire.rs", source.as_bytes()),
            ("projection.lock", &encode(&lock)?),
            ("obligations.json", &obligations),
        ],
    )
}
