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
    maximum_wire_bytes: u32,
    size_error: String,
    admission_error: String,
    constants: Vec<Constant>,
    literals: BTreeMap<String, String>,
    endpoints: Vec<Endpoint>,
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
    errors: BTreeMap<String, NativeError>,
    constants: BTreeMap<String, String>,
    fields: BTreeMap<String, String>,
    endpoints: BTreeMap<String, NativeEndpoint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    foreign_members: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    auxiliary: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeError {
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

impl Protocol {
    fn view<'a>(
        &'a self,
        schema: &'a Schema,
        binding: &Binding,
        native: &'a Native,
    ) -> Result<View<'a>> {
        if self.format != FORMAT || native.format != "oddlang-json-wire-protocol-rust-v1" {
            return Err("unsupported wire protocol or native binding".into());
        }
        iri(&self.id)?;
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
        let Some(Definition::Adjacent { variants, .. }) = schema
            .types
            .iter()
            .find(|definition| definition.id() == selector_type)
        else {
            return Err("envelope selector must refer to an adjacent union".into());
        };
        let mut ids = BTreeSet::new();
        let mut names: BTreeSet<_> = schema
            .types
            .iter()
            .filter_map(|ty| binding.names.get(ty.id()).map(String::as_str))
            .collect();
        let mut constant_ids = BTreeSet::new();
        for constant in &self.constants {
            iri(&constant.id)?;
            if !ids.insert(constant.id.as_str())
                || !literal_matches(&constant.value, &constant.shape)
            {
                return Err("invalid or duplicate protocol constant".into());
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
            iri(&endpoint.id)?;
            if !ids.insert(&endpoint.id) || !endpoint_ids.insert(endpoint.id.as_str()) {
                return Err("duplicate native endpoint identity".into());
            }
            let variant = variants
                .iter()
                .find(|item| item.id == endpoint.variant)
                .ok_or(" unknown endpoint case")?;
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
                iri(&item.id)?;
                if !ids.insert(&item.id) || !required.remove(&item.from) {
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
                    id: item.id.clone(),
                    name: name.clone(),
                    shape: shape.clone(),
                });
            }
            if !required.is_empty() || !distinct(mapped.iter().map(|f| &f.name)) {
                return Err("endpoint must map every field to one distinct binding".into());
            }
            for guard in &endpoint.guards {
                iri(&guard.id)?;
                if !ids.insert(&guard.id) {
                    return Err("duplicate operation guard identity".into());
                }
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
        for digest in &self.digests {
            digest.errors(&mut errors);
        }
        if constant_ids != native.constants.keys().map(String::as_str).collect()
            || endpoint_ids != native.endpoints.keys().map(String::as_str).collect()
            || field_ids != native.fields.keys().map(String::as_str).collect()
            || errors != native.errors.keys().map(String::as_str).collect()
        {
            return Err(
                "wire protocol target binding must cover exactly the declared API and errors"
                    .into(),
            );
        }
        for (id, error) in &native.errors {
            iri(id)?;
            if !symbol(&error.variant) {
                return Err("invalid native protocol error variant".into());
            }
        }
        Ok(View {
            envelope,
            union: selector_type,
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

    let mut wire = super::render::project_with_options(
        &shape,
        &shape_binding,
        !graph.hex_exports.is_empty(),
        &[&graph.envelope, view.union],
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
    for endpoint in &graph.endpoints {
        rows.push(serde_json::json!({
            "declaration":endpoint.id,
            "role":"canonical-frame",
            "maximumWireBytes":graph.maximum_wire_bytes,
            "selectedCase":endpoint.variant,
            "mappings":endpoint.fields,
            "decodeOrder":["wire-size","closed-parse","literals","canonical-reencode","case","validation"],
            "encodeOrder":["validation","envelope","serialize","wire-size"],
            "sizeError":graph.size_error,
            "admissionError":graph.admission_error,
            "qualification":"independent-witness-required",
        }));
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
