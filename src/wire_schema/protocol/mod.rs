//! Canonical bounded envelopes with typed, reversible native operation mappings.
use super::{
    Binding, Definition, Field, Payload, Predicate, Result, Schema, Shape, Variant,
    allowed_derives, digest, distinct, encode, iri, json, native_path, publish, supports_derive,
    symbol, type_symbol,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod digests;
mod preflight;
mod rules;
mod rust;

const FORMAT: &str = "oddlang-json-wire-protocol-v1";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Protocol {
    format: String,
    id: String,
    envelope: String,
    selector: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout: Option<Layout>,
    maximum_wire_bytes: u32,
    size_error: String,
    admission_error: String,
    errors: BTreeMap<String, Error>,
    constants: Vec<Constant>,
    literals: BTreeMap<String, String>,
    endpoints: Vec<Endpoint>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::shared_declarations::unique_map"
    )]
    record_checks: BTreeMap<String, rules::RecordCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    foreign: Vec<rules::Foreign>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<digests::Alias>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    digests: Vec<digests::Digest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hex_exports: Vec<digests::HexExport>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Layout {
    FlatSorted,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Constant {
    id: String,
    value: Value,
    #[serde(rename = "type")]
    shape: Shape,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    id: String,
    variant: String,
    fields: Vec<MappedField>,
    guards: Vec<Guard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    steps: Vec<rules::Step>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    timed: Vec<rules::Step>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MappedField {
    id: String,
    from: Source,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Source {
    EnvelopeField { field: String },
    CaseField { field: String },
    CaseValue {},
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    id: String,
    when: Option<Predicate>,
    require: Predicate,
    error: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Native {
    format: String,
    error_type: String,
    errors: BTreeMap<String, String>,
    constants: BTreeMap<String, String>,
    fields: BTreeMap<String, String>,
    endpoints: BTreeMap<String, NativeEndpoint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    foreign_members: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    auxiliary: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    internal_envelope: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Error {
    variant: String,
    argument: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeEndpoint {
    #[serde(rename = "type")]
    ty: String,
    encode: String,
    decode: String,
    derives: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timed: Option<String>,
}

struct View<'a> {
    envelope: &'a [Field],
    union: &'a str,
    flat: Option<(&'a str, &'a str, &'a str)>,
    union_cases: usize,
    endpoints: Vec<EndpointView<'a>>,
}

struct EndpointView<'a> {
    graph: &'a Endpoint,
    target: &'a NativeEndpoint,
    variant: &'a Variant,
    fields: Vec<Field>,
}

fn record<'a>(schema: &'a Schema, id: &str) -> Result<&'a [Field]> {
    match schema.types.iter().find(|ty| ty.id() == id) {
        Some(Definition::Record { fields, .. }) => Ok(fields),
        _ => Err(format!("protocol envelope must be a named record: {id}")),
    }
}

fn field<'a>(fields: &'a [Field], id: &str) -> Result<&'a Field> {
    fields
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("unknown envelope or case field {id}"))
}

fn literal_matches(value: &Value, shape: &Shape) -> bool {
    match shape {
        Shape::Text {} => value.is_string(),
        Shape::Bool {} => value.is_boolean(),
        Shape::Unsigned { bits } if matches!(bits, 8 | 16 | 32 | 64) => value
            .as_u64()
            .is_some_and(|v| v <= u64::MAX >> (64 - *bits)),
        _ => false,
    }
}

fn same_literal_type(left: &Shape, right: &Shape) -> bool {
    match (left, right) {
        (Shape::Text {}, Shape::Text {}) | (Shape::Bool {}, Shape::Bool {}) => true,
        (Shape::Unsigned { bits: left }, Shape::Unsigned { bits: right }) => left == right,
        _ => false,
    }
}

impl Native {
    fn shape_is_public(&self, shape: &Shape, graph: &Protocol, union: &str) -> bool {
        if !self.internal_envelope {
            return true;
        }
        match shape {
            Shape::Ref { id } => id != &graph.envelope && id != union,
            Shape::Optional { item } | Shape::List { item } => {
                self.shape_is_public(item, graph, union)
            }
            _ => true,
        }
    }
}

fn payload_fields(payload: &Payload) -> &[Field] {
    match payload {
        Payload::Record { fields } => fields,
        _ => &[],
    }
}

impl Protocol {
    fn validate_declarations(&self, schema: &Schema) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut add = |id: &str| -> Result<()> {
            iri(id)?;
            if !ids.insert(id.to_owned()) {
                return Err(format!("duplicate wire declaration {id}"));
            }
            Ok(())
        };
        add(&schema.id)?;
        add(&self.id)?;
        for definition in &schema.types {
            add(definition.id())?;
            match definition {
                Definition::Record { fields, .. } => {
                    for field in fields {
                        add(&field.id)?;
                    }
                }
                Definition::Adjacent { variants, .. } => {
                    for variant in variants {
                        add(&variant.id)?;
                        for field in payload_fields(&variant.payload) {
                            add(&field.id)?;
                        }
                    }
                }
                Definition::UnitEnum { .. } | Definition::External { .. } => {}
            }
        }
        for guard in &schema.guards {
            add(&guard.id)?;
        }
        for constant in &self.constants {
            add(&constant.id)?;
        }
        for error in self.errors.keys() {
            add(error)?;
        }
        for (id, check) in &self.record_checks {
            add(id)?;
            for step in &check.steps {
                for id in step.declarations() {
                    add(id)?;
                }
            }
        }
        for endpoint in &self.endpoints {
            add(&endpoint.id)?;
            for field in &endpoint.fields {
                add(&field.id)?;
            }
            for guard in &endpoint.guards {
                add(&guard.id)?;
            }
            for step in endpoint.steps.iter().chain(&endpoint.timed) {
                for id in step.declarations() {
                    add(id)?;
                }
            }
        }
        for foreign in &self.foreign {
            for field in &foreign.fields {
                add(&field.id)?;
            }
            for variant in &foreign.variants {
                add(variant)?;
            }
            for method in &foreign.methods {
                add(&method.id)?;
            }
        }
        for alias in &self.aliases {
            add(&alias.id)?;
        }
        for adapter in &self.hex_exports {
            add(&adapter.id)?;
        }
        for digest in &self.digests {
            for id in digest.declarations() {
                add(id)?;
            }
        }
        Ok(())
    }

    fn validate_visibility(&self, schema: &Schema, native: &Native, union: &str) -> Result<()> {
        if !native.internal_envelope {
            return Ok(());
        }
        let is_internal = |id: &str| id == self.envelope || id == union;
        for shape in &schema.types {
            if is_internal(shape.id()) {
                continue;
            }
            let is_public = |field: &Field| native.shape_is_public(&field.shape, self, union);
            let visible = match shape {
                Definition::Record { fields, .. } => fields.iter().all(is_public),
                Definition::Adjacent { variants, .. } => {
                    variants.iter().all(|variant| match &variant.payload {
                        Payload::Record { fields } => fields.iter().all(is_public),
                        Payload::Newtype { shape } => native.shape_is_public(shape, self, union),
                        Payload::Unit {} => true,
                    })
                }
                Definition::UnitEnum { .. } | Definition::External { .. } => true,
            };
            if !visible {
                return Err("public structural type refers to the internal frame".into());
            }
        }
        if schema.guards.iter().any(|guard| is_internal(&guard.owner)) {
            return Err("an internal frame cannot own standalone structural validation".into());
        }
        Ok(())
    }

    fn view<'a>(
        &'a self,
        schema: &'a Schema,
        binding: &Binding,
        native: &'a Native,
    ) -> Result<View<'a>> {
        if self.format != FORMAT || native.format != "oddlang-json-wire-protocol-rust-v1" {
            return Err("unsupported wire protocol or native binding".into());
        }
        self.validate_declarations(schema)?;
        if self.maximum_wire_bytes == 0
            || !native_path(&native.error_type)
            || self.endpoints.is_empty()
        {
            return Err("invalid wire limit, error binding or endpoint list".into());
        }
        let envelope = record(schema, &self.envelope)?;
        let selector = field(envelope, &self.selector)?;
        let Shape::Ref { id: selector_type } = &selector.shape else {
            return Err("envelope selector must refer to an adjacent union".into());
        };
        let Some(Definition::Adjacent {
            variants,
            tag,
            content,
            ..
        }) = schema
            .types
            .iter()
            .find(|definition| definition.id() == selector_type)
        else {
            return Err("envelope selector must refer to an adjacent union".into());
        };
        let flat = self
            .layout
            .as_ref()
            .map(|_| (selector.name.as_str(), tag.as_str(), content.as_str()));
        if flat.is_some()
            && envelope
                .iter()
                .any(|f| &f.name == tag || &f.name == content)
        {
            return Err("flat wire field collision".into());
        }
        self.validate_visibility(schema, native, selector_type)?;
        let mut names: BTreeSet<_> = schema
            .types
            .iter()
            .filter_map(|ty| binding.names.get(ty.id()).map(String::as_str))
            .collect();
        let mut constant_ids = BTreeSet::new();
        for constant in &self.constants {
            if !literal_matches(&constant.value, &constant.shape) {
                return Err("invalid protocol constant".into());
            }
            constant_ids.insert(constant.id.as_str());
            let name = native
                .constants
                .get(&constant.id)
                .ok_or("missing native constant name")?;
            if !symbol(name)
                || !name.bytes().all(|ch| !ch.is_ascii_lowercase())
                || !names.insert(name)
            {
                return Err("invalid or duplicate native constant name".into());
            }
        }
        for (id, value) in &self.literals {
            if id == &self.selector {
                return Err("selector cannot also be a literal".into());
            }
            let target = &field(envelope, id)?.shape;
            let constant = self
                .constants
                .iter()
                .find(|item| &item.id == value)
                .ok_or("unknown envelope literal")?;
            if !same_literal_type(target, &constant.shape) {
                return Err("envelope literal type mismatch".into());
            }
        }
        let required_envelope: BTreeSet<_> = envelope
            .iter()
            .filter(|f| f.id != self.selector && !self.literals.contains_key(&f.id))
            .map(|f| Source::EnvelopeField {
                field: f.id.clone(),
            })
            .collect();
        let mut endpoint_ids = BTreeSet::new();
        let mut field_ids = BTreeSet::new();
        let mut errors = BTreeSet::from([self.size_error.as_str(), self.admission_error.as_str()]);
        let mut endpoints = Vec::new();
        for endpoint in &self.endpoints {
            endpoint_ids.insert(endpoint.id.as_str());
            let variant = variants
                .iter()
                .find(|item| item.id == endpoint.variant)
                .ok_or("unknown endpoint case")?;
            let target = native
                .endpoints
                .get(&endpoint.id)
                .ok_or("missing native endpoint binding")?;
            if !type_symbol(&target.ty)
                || !symbol(&target.encode)
                || !symbol(&target.decode)
                || ![&target.ty, &target.encode, &target.decode]
                    .into_iter()
                    .all(|name| names.insert(name))
                || !allowed_derives(&target.derives)
            {
                return Err("invalid native endpoint API name or derive".into());
            }
            if target.timed.is_some() == endpoint.timed.is_empty()
                || target.timed.as_deref().is_some_and(|name| !symbol(name))
            {
                return Err("timed guards require one native method binding".into());
            }
            for step in endpoint.steps.iter().chain(&endpoint.timed) {
                step.errors(&mut errors);
            }
            let mut required = required_envelope.clone();
            match &variant.payload {
                Payload::Record { fields } => {
                    required.extend(fields.iter().map(|f| Source::CaseField {
                        field: f.id.clone(),
                    }))
                }
                Payload::Newtype { .. } => {
                    required.insert(Source::CaseValue {});
                }
                Payload::Unit {} => {}
            }
            let mut mapped = Vec::new();
            for item in &endpoint.fields {
                if !required.remove(&item.from) {
                    return Err(
                        "endpoint must map each envelope and case value exactly once".into(),
                    );
                }
                field_ids.insert(item.id.as_str());
                let name = native
                    .fields
                    .get(&item.id)
                    .ok_or("missing native endpoint field")?;
                if !symbol(name) {
                    return Err("invalid native endpoint field name".into());
                }
                let shape = match &item.from {
                    Source::EnvelopeField { field: id } => &field(envelope, id)?.shape,
                    Source::CaseField { field: id } => {
                        let Payload::Record { fields } = &variant.payload else {
                            unreachable!("checked mapping")
                        };
                        &field(fields, id)?.shape
                    }
                    Source::CaseValue {} => {
                        let Payload::Newtype { shape } = &variant.payload else {
                            unreachable!("checked mapping")
                        };
                        shape
                    }
                };
                if !native.shape_is_public(shape, self, selector_type) {
                    return Err("public native endpoint field refers to the internal frame".into());
                }
                if !supports_derive(shape, "Clone", binding)
                    || target
                        .derives
                        .iter()
                        .any(|derive| !supports_derive(shape, derive, binding))
                {
                    return Err(
                        "mapped endpoint field must support Clone and requested derives".into(),
                    );
                }
                mapped.push(Field {
                    require_presence: false,
                    id: item.id.clone(),
                    name: name.clone(),
                    shape: shape.clone(),
                });
            }
            if !required.is_empty() || !distinct(mapped.iter().map(|f| &f.name)) {
                return Err("endpoint must map every field to one distinct binding".into());
            }
            for guard in &endpoint.guards {
                guard.require.validate(&mapped)?;
                if let Some(when) = &guard.when {
                    when.validate(&mapped)?;
                }
                errors.insert(&guard.error);
            }
            endpoints.push(EndpointView {
                graph: endpoint,
                target,
                variant,
                fields: mapped,
            });
        }
        for check in self.record_checks.values() {
            for step in &check.steps {
                step.errors(&mut errors);
            }
        }
        for digest in &self.digests {
            digest.errors(&mut errors);
        }
        if constant_ids != native.constants.keys().map(String::as_str).collect()
            || endpoint_ids != native.endpoints.keys().map(String::as_str).collect()
            || field_ids != native.fields.keys().map(String::as_str).collect()
            || errors != self.errors.keys().map(String::as_str).collect()
        {
            return Err(
                "wire protocol target binding must cover exactly the declared API and errors"
                    .into(),
            );
        }
        let mut error_variants = BTreeMap::new();
        for (id, error) in &self.errors {
            iri(id)?;
            iri(&error.variant)?;
            let takes_text = error.argument.is_some();
            if error_variants
                .insert(error.variant.as_str(), takes_text)
                .is_some_and(|previous| previous != takes_text)
            {
                return Err("one protocol error variant cannot mix unit and text arguments".into());
            }
        }
        if error_variants
            .keys()
            .copied()
            .ne(native.errors.keys().map(String::as_str))
            || !native.errors.values().all(|variant| symbol(variant))
            || !distinct(native.errors.values())
        {
            return Err(
                "native error variants must cover the declared protocol error roles".into(),
            );
        }
        Ok(View {
            envelope,
            union: selector_type,
            flat,
            union_cases: variants.len(),
            endpoints,
        })
    }
}

pub fn build(
    shape: &Path,
    shape_binding: &Path,
    graph: &Path,
    binding: &Path,
    destination: &Path,
) -> Result<()> {
    let mut shape: Schema = json(shape)?;
    shape.types.sort_by(|left, right| left.id().cmp(right.id()));
    let shape_binding: Binding = json(shape_binding)?;
    shape.validate(&shape_binding)?;
    let graph: Protocol = json(graph)?;
    let binding: Native = json(binding)?;
    let view = graph.view(&shape, &shape_binding, &binding)?;

    let internal = [&*graph.envelope, view.union];
    let internal = if binding.internal_envelope {
        &internal[..]
    } else {
        &[]
    };
    let mut wire = super::render::project_with_options(
        &shape,
        &shape_binding,
        !graph.hex_exports.is_empty(),
        internal,
    );
    wire.push_str(&rust::render(
        &graph,
        &binding,
        &shape,
        &shape_binding,
        &view,
    )?);
    let mut obligations = super::obligations::derive(&shape);
    let rows = obligations["obligations"]
        .as_array_mut()
        .expect("shape obligations");
    for (id, check) in &graph.record_checks {
        rows.push(
            serde_json::json!({"declaration":id,"role":"record-check","owner":check.owner,
            "qualification":"independent-witness-required"}),
        );
        for step in &check.steps {
            step.visit(&mut |item| rows.push(serde_json::json!({"declaration":item.id(),
                "role":"admission","recordCheck":id,"step":item,"qualification":"independent-witness-required"})));
        }
    }
    for endpoint in &graph.endpoints {
        rows.push(serde_json::json!({
            "declaration":endpoint.id,
            "role":"canonical-frame",
            "maximumWireBytes":graph.maximum_wire_bytes,
            "envelope":graph.envelope,
            "selector":graph.selector,
            "literals":graph.literals,
            "selectedCase":endpoint.variant,
            "mappings":endpoint.fields,
            "decodeOrder":["wire-size","closed-parse","literals","canonical-reencode","case","validation"],
            "encodeOrder":["validation","borrowed-shape-bounds","envelope","bounded-serialize"],
            "sizeError":graph.size_error,
            "admissionError":graph.admission_error,
            "qualification":"independent-witness-required",
        }));
        if let Some(layout) = &graph.layout {
            let frame = rows.last_mut().expect("frame obligation");
            frame["layout"] = serde_json::json!(layout);
            frame["encoding"] = serde_json::json!({
                "objects": "recursive-utf8-key-order",
                "json": "compact-utf8-serde-json-escaping",
                "selector": "promote-tag-and-content",
                "rejectNestedSelector": true,
                "caseDigests": "independent-declared-order"
            });
        }
        for (phase, steps) in [("admission", &endpoint.steps), ("clock", &endpoint.timed)] {
            for step in steps {
                step.visit(&mut |step| {
                    rows.push(serde_json::json!({"declaration":step.id(),"role":phase,
                        "endpoint":endpoint.id,"step":step,"qualification":"independent-witness-required"}));
                });
            }
        }
        for guard in &endpoint.guards {
            rows.push(serde_json::json!({"declaration":guard.id,"role":"validation",
                "endpoint":endpoint.id,"predicate":guard.require,"when":guard.when,"error":guard.error,
                "qualification":"independent-witness-required"}));
        }
    }
    for (id, error) in &graph.errors {
        rows.push(serde_json::json!({"declaration":id,"role":"named-error",
            "error":error,"qualification":"independent-witness-required"}));
    }
    for constant in &graph.constants {
        rows.push(
            serde_json::json!({"declaration":constant.id,"role":"literal",
            "value":constant.value,"qualification":"independent-witness-required"}),
        );
    }
    for alias in &graph.aliases {
        rows.push(
            serde_json::json!({"declaration":alias.id,"role":"fixed-byte-binding",
            "bytes":alias.bytes,"qualification":"independent-witness-required"}),
        );
    }
    for foreign in &graph.foreign {
        rows.push(
            serde_json::json!({"declaration":foreign.id,"role":"foreign-dependency",
            "capabilities":foreign,"qualification":"independent-witness-required"}),
        );
    }
    for digest in &graph.digests {
        rows.push(
            serde_json::json!({"declaration":digest.id(),"role":"sha256",
            "operation":digest,"qualification":"independent-witness-required"}),
        );
        if let digests::Digest::Case { steps, .. } = digest {
            for step in steps {
                step.visit(&mut |step| {
                    rows.push(serde_json::json!({"declaration":step.id(),"role":"digest-validation",
                        "operation":digest.id(),"step":step,"qualification":"independent-witness-required"}));
                });
            }
        }
    }
    for role in &graph.hex_exports {
        rows.push(
            serde_json::json!({"declaration":role.id,"role":"wire-adapter",
            "adapter":role,"qualification":"independent-witness-required"}),
        );
    }
    let obligations = encode(&obligations)?;
    let lock = encode(&serde_json::json!({
        "format":FORMAT,
        "shapeSha256":digest(&encode(&shape)?),
        "shapeBindingSha256":digest(&encode(&shape_binding)?),
        "protocolSha256":digest(&encode(&graph)?),
        "protocolBindingSha256":digest(&encode(&binding)?),
        "wireSha256":digest(wire.as_bytes()),
        "obligationsSha256":digest(&obligations),
    }))?;
    publish(
        destination,
        &[
            ("wire.rs", wire.as_bytes()),
            ("projection.lock", &lock),
            ("obligations.json", &obligations),
        ],
    )
}
