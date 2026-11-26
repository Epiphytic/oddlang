//! Portable checked value records and finite representation tables.
mod fixed_recovery;
mod recover_choice;
pub use fixed_recovery::recover_fixed_bytes;
pub(crate) use recover_choice::{additional_binding, plain_choice_cases};
pub use recover_choice::recover;
pub(crate) use rust::render_plain_choice;
pub use typescript::build as build_typescript;
mod rust;
mod typescript;
pub use typescript::build as build_typescript;
mod typescript;

use crate::{
    native_package::publish,
    shared_declarations::{checked_method_name, unique_map},
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
const FORMAT: &str = "oddlang-checked-values-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    #[serde(deserialize_with = "unique_map")]
    types: BTreeMap<String, Definition>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Equality {
    None,
    Partial,
    #[default]
    Total,
}

impl Equality {
    fn is_total(&self) -> bool {
        *self == Self::Total
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Definition {
    Choice {
        cases: Vec<String>,
        #[serde(default, skip_serializing_if = "Equality::is_total")]
        equality: Equality,
        #[serde(default = "default_copy", skip_serializing_if = "is_copy")]
        copy: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        debug: bool,
    },
    #[serde(rename = "fixed-bytes")]
    FixedBytes {
        length: u32,
        failure: String,
        #[serde(rename = "onDrop")]
        on_drop: DropPolicy,
    },
    Enum {
        #[serde(deserialize_with = "unique_map")]
        values: BTreeMap<String, Literal>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure: Option<String>,
    },
    Record {
        #[serde(deserialize_with = "unique_map")]
        fields: BTreeMap<String, Field>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        constructor: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        guards: Vec<Guard>,
    },
}

fn default_copy() -> bool {
    true
}

fn is_copy(copy: &bool) -> bool {
    *copy
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum DropPolicy {
    Erase,
}

#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(untagged)]
enum Literal {
    U8(u8),
    Text(String),
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Field {
    Text {},
    U8 {},
    Named { id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Guard {
    OneOf {
        field: String,
        values: Vec<u8>,
    },
    Utf8ByteLength {
        field: String,
        minimum: u32,
        maximum: u32,
    },
    AsciiBytes {
        field: String,
        ranges: Vec<Range>,
    },
    ExcludeUnicode {
        field: String,
        #[serde(rename = "generalCategoryCc")]
        controls: bool,
        ranges: Vec<Range>,
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Range {
    minimum: u32,
    maximum: u32,
}

impl Guard {
    fn field(&self) -> &str {
        match self {
            Self::OneOf { field, .. }
            | Self::Utf8ByteLength { field, .. }
            | Self::AsciiBytes { field, .. }
            | Self::ExcludeUnicode { field, .. } => field,
        }
    }

    fn check(&self, fields: &BTreeMap<String, Field>) -> bool {
        fn intervals(ranges: &[Range], limit: u32) -> bool {
            ranges
                .iter()
                .all(|r| r.minimum <= r.maximum && r.maximum <= limit)
                && ranges
                    .windows(2)
                    .all(|pair| pair[0].maximum < pair[1].minimum)
        }
        let field = fields.get(self.field());
        match self {
            Self::OneOf { values, .. } => {
                matches!(field, Some(Field::U8 {}))
                    && !values.is_empty()
                    && values.windows(2).all(|pair| pair[0] < pair[1])
            }
            Self::Utf8ByteLength {
                minimum, maximum, ..
            } => matches!(field, Some(Field::Text {})) && minimum <= maximum,
            Self::AsciiBytes { ranges, .. } => {
                matches!(field, Some(Field::Text {}))
                    && !ranges.is_empty()
                    && intervals(ranges, 0x7f)
            }
            Self::ExcludeUnicode {
                controls, ranges, ..
            } => {
                matches!(field, Some(Field::Text {}))
                    && (*controls || !ranges.is_empty())
                    && intervals(ranges, 0x10ffff)
                    && ranges
                        .iter()
                        .all(|r| r.maximum < 0xd800 || r.minimum > 0xdfff)
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    #[serde(deserialize_with = "unique_map")]
    types: BTreeMap<String, Native>,
    #[serde(deserialize_with = "unique_map")]
    variants: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    errors: BTreeMap<String, Fault>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Native {
    name: String,
    order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    traits: Vec<Derived>,
    view: Option<String>,
    parse: Option<String>,
    shape: Option<Shape>,
    constructor: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "unique_map"
    )]
    members: BTreeMap<String, Member>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Derived {
    Copy,
    Hash,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Shape {
    Tuple,
    Named,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Member {
    name: String,
    getter: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    public: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fault {
    error_type: String,
    variant: String,
}

impl Fault {
    fn path(&self) -> String {
        format!("{}::{}", self.error_type, self.variant)
    }
}

fn declare<'a>(seen: &mut BTreeSet<&'a str>, id: &'a str) -> Result<()> {
    iri(id)?;
    if !seen.insert(id) {
        return Err(format!("repeated checked value identity: {id}"));
    }
    Ok(())
}

fn optional_symbol(value: Option<&str>) -> bool {
    value.is_none_or(checked_method_name)
}

fn order(keys: impl Iterator<Item = String>, values: &[String]) -> bool {
    let keys: BTreeSet<_> = keys.collect();
    keys.len() == values.len() && keys == values.iter().cloned().collect()
}

impl Graph {
    fn validate(&self, bindings: &Binding) -> Result<()> {
        if self.format != FORMAT
            || bindings.format != "oddlang-checked-values-rust-v1"
            || self.types.is_empty()
            || !self.types.keys().eq(bindings.types.keys())
        {
            return Err("checked values require exact format and native type bindings".into());
        }
        let mut ids = BTreeSet::new();
        declare(&mut ids, &self.id)?;
        let mut variants = BTreeSet::new();
        let mut errors = BTreeSet::new();
        let mut names = BTreeSet::new();
        for (id, definition) in &self.types {
            declare(&mut ids, id)?;
            let native = &bindings.types[id];
            if !type_symbol(&native.name)
                || !names.insert(&native.name)
                || native.traits.iter().collect::<BTreeSet<_>>().len() != native.traits.len()
            {
                return Err(format!("invalid checked type name or traits: {id}"));
            }
            match definition {
                Definition::FixedBytes { failure, .. } => {
                    let methods = [&native.constructor, &native.parse, &native.view];
                    let mut names = BTreeSet::new();
                    if !methods.into_iter().all(|method| {
                        method
                            .as_deref()
                            .is_some_and(|name| checked_method_name(name) && names.insert(name))
                    }) || !native.order.is_empty()
                        || !native.traits.is_empty()
                        || !native.members.is_empty()
                        || native.shape.is_some()
                    {
                        return Err(format!("invalid fixed-byte bindings: {id}"));
                    }
                    errors.insert(failure);
                }
                Definition::Choice { cases, copy, .. } => {
                    if cases.is_empty()
                        || native.order != *cases
                        || (if *copy {
                            native.traits != [Derived::Copy]
                        } else {
                            !native.traits.is_empty()
                        })
                        || native.view.is_some()
                        || native.parse.is_some()
                        || native.shape.is_some()
                        || native.constructor.is_some()
                        || !native.members.is_empty()
                    {
                        return Err(format!("invalid plain choice native binding: {id}"));
                    }
                    let mut labels = BTreeSet::new();
                    for case in cases {
                        declare(&mut ids, case)?;
                        variants.insert(case);
                        if !bindings
                            .variants
                            .get(case)
                            .is_some_and(|name| type_symbol(name) && labels.insert(name))
                        {
                            return Err(format!("invalid or duplicate plain choice case: {case}"));
                        }
                    }
                }
                Definition::Enum { values, failure } => {
                    if values.is_empty()
                        || !order(values.keys().cloned(), &native.order)
                        || !native.view.as_deref().is_some_and(checked_method_name)
                        || !optional_symbol(native.parse.as_deref())
                        || native.parse.is_some() != failure.is_some()
                        || (native.parse.is_some() && native.parse == native.view)
                        || native.shape.is_some()
                        || native.constructor.is_some()
                        || !native.members.is_empty()
                    {
                        return Err(format!("invalid finite value table bindings: {id}"));
                    }
                    errors.extend(failure);
                    let mut labels = BTreeSet::new();
                    let mut repr = BTreeSet::new();
                    let first = values.values().next().expect("table cannot be empty");
                    for (case, literal) in values {
                        declare(&mut ids, case)?;
                        variants.insert(case);
                        if !bindings
                            .variants
                            .get(case)
                            .is_some_and(|v| type_symbol(v) && labels.insert(v))
                            || std::mem::discriminant(first) != std::mem::discriminant(literal)
                            || !repr.insert(rust::literal(literal))
                        {
                            return Err(format!("invalid or repeated representation: {case}"));
                        }
                    }
                }
                Definition::Record {
                    fields,
                    constructor,
                    guards,
                } => {
                    if fields.is_empty()
                        || !order(fields.keys().cloned(), &native.order)
                        || !fields.keys().eq(native.members.keys())
                        || !optional_symbol(native.constructor.as_deref())
                        || native.constructor.is_some() != constructor.is_some()
                        || (constructor.is_none()
                            && (!guards.is_empty()
                                || native.members.values().any(|member| !member.public)))
                        || native.shape.is_none()
                        || native.view.is_some()
                        || native.parse.is_some()
                        || (native.shape == Some(Shape::Tuple) && fields.len() != 1)
                    {
                        return Err(format!("invalid checked record bindings: {id}"));
                    }
                    errors.extend(constructor);
                    let mut field_names = BTreeSet::new();
                    let mut methods: BTreeSet<_> = native.constructor.iter().collect();
                    for (field_id, field) in fields {
                        declare(&mut ids, field_id)?;
                        let target = &native.members[field_id];
                        if !symbol(&target.name)
                            || !field_names.insert(&target.name)
                            || target.getter.as_ref().is_some_and(|name| {
                                !checked_method_name(name) || !methods.insert(name)
                            })
                            || (constructor.is_some() && target.public)
                            || (!target.public && target.getter.is_none())
                        {
                            return Err(format!("invalid checked field binding: {field_id}"));
                        }
                        if let Field::Named { id } = field {
                            match self.types.get(id) {
                                Some(Definition::Choice { .. }) => {
                                    return Err(format!("checked record field lacks Debug: {id}"));
                                }
                                Some(Definition::FixedBytes { .. }) | None => {
                                    return Err(format!(
                                        "unresolved or unsupported checked field type: {id}"
                                    ));
                                }
                                _ => {}
                            }
                        }
                        for capability in &native.traits {
                            let supports = match field {
                                Field::Text {} => *capability != Derived::Copy,
                                Field::U8 {} => true,
                                Field::Named { id } => {
                                    bindings.types[id].traits.contains(capability)
                                }
                            };
                            if !supports {
                                return Err(format!(
                                    "field lacks a required derived capability: {field_id}"
                                ));
                            }
                        }
                    }
                    if !guards.iter().all(|guard| guard.check(fields)) {
                        return Err(format!("unsupported or ill-typed constructor guard: {id}"));
                    }
                }
            }
        }
        if !variants.into_iter().eq(bindings.variants.keys())
            || !errors.into_iter().eq(bindings.errors.keys())
        {
            return Err("checked values require exact error and variant bindings".into());
        }
        for (id, fault) in &bindings.errors {
            declare(&mut ids, id)?;
            if !fault
                .error_type
                .strip_prefix("crate::")
                .is_some_and(|tail| {
                    tail.split("::").all(symbol)
                        && tail.rsplit("::").next().is_some_and(type_symbol)
                })
                || !type_symbol(&fault.variant)
            {
                return Err(format!("invalid checked error binding: {id}"));
            }
        }
        self.check_types()
    }

    fn check_types(&self) -> Result<()> {
        fn visit<'a>(
            graph: &'a Graph,
            id: &'a str,
            active: &mut BTreeSet<&'a str>,
            complete: &mut BTreeSet<&'a str>,
        ) -> Result<()> {
            if complete.contains(id) {
                return Ok(());
            }
            if !active.insert(id) || active.len() > 64 {
                return Err(format!(
                    "recursive or excessive checked value nesting: {id}"
                ));
            }
            if let Definition::Record { fields, .. } = &graph.types[id] {
                for field in fields.values() {
                    if let Field::Named { id } = field {
                        visit(graph, id, active, complete)?;
                    }
                }
            }
            active.remove(id);
            complete.insert(id);
            Ok(())
        }
        let mut checked = BTreeSet::new();
        for id in self.types.keys() {
            visit(self, id, &mut BTreeSet::new(), &mut checked)?;
        }
        Ok(())
    }
}

pub(crate) fn needs_erasure_provider(bytes: &[u8]) -> Result<bool> {
    let graph: Graph = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    Ok(graph
        .types
        .values()
        .any(|ty| matches!(ty, Definition::FixedBytes { .. })))
}

pub(crate) fn validate_error_links(
    overlay: &[u8],
    providers: &BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    let binding: Binding = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    for (id, fault) in binding.errors {
        let path = fault.path();
        if !providers
            .get(&id)
            .is_some_and(|paths| paths.contains(&path))
        {
            return Err(format!(
                "checked value error {id} has no matching unit export at {path}"
            ));
        }
    }
    Ok(())
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    let graph: Graph = json(graph)?;
    let mut bindings: Binding = json(overlay)?;
    graph.validate(&bindings)?;
    for native in bindings.types.values_mut() {
        native.traits.sort();
    }
    let source = rust::render(&graph, &bindings);
    let obligations = encode(&obligations(&graph))?;
    let lock = encode(&json!({
        "format":"oddlang-checked-values-lock-v1","domainSha256":digest(&encode(&graph)?),"bindingSha256":digest(&encode(&bindings)?),
        "sourceSha256":digest(source.as_bytes()),"obligationsSha256":digest(&obligations),
    }))?;
    publish_projection("values.rs", &source, &obligations, &lock, output)
}

fn obligations(graph: &Graph) -> serde_json::Value {
    json!({
        "format":FORMAT,"domain":graph.id,
        "obligations":graph.types.iter().map(|(id,definition)| {
            let expected = match definition {
                Definition::FixedBytes { length, failure, on_drop } =>
                    json!({"length":length,"failure":failure,"onDrop":on_drop,"view":"borrow","input":"exact-bytes"}),
                Definition::Choice { cases, equality, copy, debug } => {
                    let mut expected = json!({
                        "orderedCases":cases,"copy":copy,"equality":match equality {
                            Equality::None => "none",
                            Equality::Partial => "partial-case-identity",
                            Equality::Total => "case-identity",
                        },
                        "externalRepresentation":null, "targetOrdinals":"zero-based-declaration-order"
                    });
                    if *debug { expected["debug"] = json!("derived-variant-name"); }
                    expected
                },
                Definition::Enum { values, failure } => json!({"values":values,"failure":failure}),
                Definition::Record { fields, constructor, guards } =>
                    json!({"fields":fields,"constructor":constructor,"orderedGuards":guards}),
            };
            json!({"declaration":id,"role":"checked-value-domain","expected":expected,
                   "qualification":"independent-witness-required"})
        }).collect::<Vec<_>>()
    })
}

fn publish_projection(
    filename: &str,
    source: &str,
    obligations: &[u8],
    lock: &[u8],
    output: &Path,
) -> Result<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|err| err.to_string())?;
    for (name, bytes) in [
        (filename, source.as_bytes()),
        ("obligations.json", obligations),
        ("projection.lock", lock),
    ] {
        fs::write(stage.path().join(name), bytes).map_err(|err| err.to_string())?;
    }
    publish(stage.path(), output)
}
