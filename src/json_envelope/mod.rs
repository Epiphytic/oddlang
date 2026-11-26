//! Bounded JSON envelopes with explicit field order and decode precedence.

use crate::model::{allowed, core, instances, iri, literal, load_graph, named, one};
use oxigraph::model::{
    Graph, NamedOrBlankNodeRef,
    vocab::{rdf, xsd},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod rust;

type Result<T> = std::result::Result<T, String>;

const LIMIT_PROPERTIES: [&str; 3] = ["rawBytes", "wireBytes", "recordLineBytes"];
const ERROR_PROPERTIES: [&str; 4] = ["sizeError", "jsonError", "literalError", "encodingError"];
const ENCODE: &[Guard] = &[Guard::RawSize, Guard::Json, Guard::WireSize];
const DECODE: &[Guard] = &[
    Guard::WireSize,
    Guard::Json,
    Guard::Literal,
    Guard::EncodedSize,
    Guard::Base64,
    Guard::RawSize,
    Guard::Canonical,
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Guard {
    WireSize,
    Json,
    Literal,
    EncodedSize,
    Base64,
    RawSize,
    Canonical,
}

#[derive(Serialize)]
struct Envelope {
    id: String,
    accept_tuple: bool,
    fields: Vec<Field>,
    limits: [u32; 3],
    encode: Vec<Guard>,
    decode: Vec<Guard>,
    errors: BTreeMap<String, String>,
    error_ids: [String; 4],
}

#[derive(Serialize)]
struct Field {
    id: String,
    name: String,
    position: u64,
    kind: FieldKind,
}

#[derive(Serialize)]
enum FieldKind {
    UnsignedLiteral { bits: u64, value: u64 },
    CanonicalBase64,
}

fn link(graph: &Graph, subject: &str, property: &str) -> Result<String> {
    iri(one(graph, &named(subject), property, subject)?, subject)
}

fn text(graph: &Graph, subject: &str, property: &str) -> Result<String> {
    literal(
        one(graph, &named(subject), property, subject)?,
        xsd::STRING,
        subject,
    )
}

fn integer(graph: &Graph, subject: &str, property: &str) -> Result<u64> {
    literal(
        one(graph, &named(subject), property, subject)?,
        xsd::INTEGER,
        subject,
    )?
    .parse()
    .map_err(|_| format!("{subject}: {property} requires unsigned integer"))
}

fn guards(graph: &Graph, id: &str, prop: &str, expected: &[Guard]) -> Result<Vec<Guard>> {
    let order = text(graph, id, prop)?
        .split_ascii_whitespace()
        .map(|token| {
            serde_json::from_value(serde_json::Value::String(token.to_owned()))
                .map_err(|_| format!("{id}: unknown guard {token} in {prop}"))
        })
        .collect::<Result<Vec<_>>>()?;
    if order.len() != expected.len()
        || order.iter().collect::<BTreeSet<_>>() != expected.iter().collect()
    {
        return Err(format!(
            "{id}: {prop} requires each applicable guard exactly once"
        ));
    }
    Ok(order)
}

fn load(root: &Path) -> Result<Envelope> {
    let (graph, _) = load_graph(root)?;
    let envelopes = instances(&graph, "JsonEnvelope")?;
    let [id] = envelopes
        .into_iter()
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "expected exactly one JsonEnvelope")?;
    let fields = instances(&graph, "JsonEnvelopeField")?;
    let errors = instances(&graph, "NamedError")?;
    for triple in graph.iter() {
        let NamedOrBlankNodeRef::NamedNode(subject) = triple.subject else {
            return Err("JSON envelope requires named declarations".into());
        };
        let name = subject.as_str();
        let kind = if name == id {
            "JsonEnvelope"
        } else if fields.contains(name) {
            "JsonEnvelopeField"
        } else if errors.contains(name) {
            "NamedError"
        } else {
            return Err(format!("{name}: undeclared subject"));
        };
        if triple.predicate == rdf::TYPE && triple.object != core(kind).as_ref().into() {
            return Err(format!("{name}: unexpected declaration type"));
        }
    }

    let mut properties = vec!["shape", "field", "encodeOrder", "decodeOrder"];
    properties.extend(LIMIT_PROPERTIES);
    properties.extend(ERROR_PROPERTIES);
    allowed(&graph, &named(&id), &properties, &id)?;

    let accept_tuple = match link(&graph, &id, "shape")? {
        shape if shape == core("ClosedRequiredObject").as_str() => false,
        shape if shape == core("ClosedRequiredObjectOrTuple").as_str() => true,
        shape => return Err(format!("{id}: unsupported JSON shape {shape}")),
    };
    let field_ids = graph
        .objects_for_subject_predicate(&named(&id), &core("field"))
        .map(|term| iri(term, &id))
        .collect::<Result<BTreeSet<_>>>()?;
    if fields.len() != 2 || field_ids != fields {
        return Err(format!("{id}: link exactly two declared envelope fields"));
    }
    let mut fields = fields
        .into_iter()
        .map(|field| load_field(&graph, field))
        .collect::<Result<Vec<_>>>()?;
    fields.sort_by_key(|field| field.position);
    if fields[0].name == fields[1].name
        || fields[0].position != 0
        || fields[1].position != 1
        || fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::UnsignedLiteral { .. }))
            .count()
            != 1
    {
        return Err(format!(
            "{id}: expected distinct ordered literal and binary fields"
        ));
    }

    let limits = LIMIT_PROPERTIES.map(|prop| {
        u32::try_from(integer(&graph, &id, prop)?).map_err(|_| format!("{id}: {prop} exceeds u32"))
    });
    let [raw, wire, line] = limits;
    let limits = [raw?, wire?, line?];
    if limits[0].div_ceil(3).checked_mul(4).is_none() || limits[1] == 0 || limits[2] < limits[1] {
        return Err(format!("{id}: inconsistent JSON and byte size limits"));
    }

    let encode = guards(&graph, &id, "encodeOrder", ENCODE)?;
    if encode != ENCODE {
        return Err(format!(
            "{id}: encode must bound raw bytes before JSON and wire size"
        ));
    }
    let decode = guards(&graph, &id, "decodeOrder", DECODE)?;
    let at = |guard| {
        decode
            .iter()
            .position(|g| *g == guard)
            .expect("complete guard set")
    };
    if decode[..2] != DECODE[..2]
        || at(Guard::EncodedSize) > at(Guard::Base64)
        || at(Guard::Base64) > at(Guard::RawSize)
        || at(Guard::Base64) > at(Guard::Canonical)
    {
        return Err(format!("{id}: unsafe or impossible decode guard order"));
    }

    let messages = errors
        .into_iter()
        .map(|error| {
            allowed(&graph, &named(&error), &["message"], &error)?;
            let message = text(&graph, &error, "message")?;
            Ok((error, message))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let [size, json, version, encoding] = ERROR_PROPERTIES.map(|prop| link(&graph, &id, prop));
    let error_ids = [size?, json?, version?, encoding?];
    if error_ids.iter().collect::<BTreeSet<_>>() != messages.keys().collect() {
        return Err(format!(
            "{id}: resolve each error role and each declared error"
        ));
    }
    Ok(Envelope {
        id,
        accept_tuple,
        fields,
        limits,
        encode,
        decode,
        errors: messages,
        error_ids,
    })
}

fn load_field(graph: &Graph, id: String) -> Result<Field> {
    let name = text(graph, &id, "wireName")?;
    if name.is_empty() {
        return Err(format!("{id}: empty JSON field name"));
    }
    let position = integer(graph, &id, "position")?;
    let mut properties = vec!["wireName", "position", "fieldKind"];
    let kind = match link(graph, &id, "fieldKind")? {
        kind if kind == core("UnsignedLiteral").as_str() => {
            properties.extend(["bits", "value"]);
            let bits = integer(graph, &id, "bits")?;
            let value = integer(graph, &id, "value")?;
            if !matches!(bits, 8 | 16 | 32 | 64) || value > u64::MAX >> (64 - bits) {
                return Err(format!("{id}: invalid unsigned literal or width"));
            }
            FieldKind::UnsignedLiteral { bits, value }
        }
        kind if kind == core("CanonicalBase64").as_str() => FieldKind::CanonicalBase64,
        kind => return Err(format!("{id}: unsupported JSON field kind {kind}")),
    };
    allowed(graph, &named(&id), &properties, &id)?;
    Ok(Field {
        id,
        name,
        position,
        kind,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    domain: String,
    codec_type: String,
    limits_type: String,
    error_type: String,
    encode: String,
    decode: String,
    constants: Constants,
    variants: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variant_order: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Constants {
    raw_bytes: String,
    wire_bytes: String,
    record_line_bytes: String,
}

impl Binding {
    fn validate(&self, envelope: &Envelope) -> Result<()> {
        if self.domain != envelope.id || !self.variants.keys().eq(envelope.errors.keys()) {
            return Err("binding must name the envelope and every error exactly".into());
        }
        if let Some(order) = &self.variant_order {
            let ids: BTreeSet<_> = order.iter().collect();
            if ids.len() != order.len() || ids != self.variants.keys().collect() {
                return Err("variantOrder must list every error IRI exactly once".into());
            }
        }
        if [&self.codec_type, &self.limits_type, &self.error_type]
            .iter()
            .any(|name| name.as_str() == "oddlang_json_record")
        {
            return Err("Rust API type name conflicts with the generated Rust module".into());
        }
        let groups = [
            vec![&self.codec_type, &self.limits_type, &self.error_type],
            vec![
                &self.constants.raw_bytes,
                &self.constants.wire_bytes,
                &self.constants.record_line_bytes,
            ],
            vec![&self.encode, &self.decode],
            self.variants.values().collect(),
        ];
        for names in groups {
            let mut seen = BTreeSet::new();
            for name in names {
                if !name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || name == "gen"
                    || syn::parse_str::<syn::Ident>(name).is_err()
                    || !seen.insert(name)
                {
                    return Err(format!("invalid or duplicate Rust API name {name:?}"));
                }
            }
        }
        Ok(())
    }
}

pub fn build(root: &Path, overlay: &Path, output: &Path) -> Result<()> {
    let envelope = load(root)?;
    let binding: Binding = serde_json::from_slice(&fs::read(overlay).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    binding.validate(&envelope)?;
    let source = rust::render(&envelope, &binding);
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let canonical_domain = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    let canonical_binding = serde_json::to_vec(&binding).map_err(|e| e.to_string())?;
    let lock = serde_json::json!({
        "format": "oddlang-json-envelope-v1",
        "domainSha256": digest(&canonical_domain),
        "bindingSha256": digest(&canonical_binding),
        "artifact": "codec.rs",
        "artifactSha256": digest(source.as_bytes()),
    });
    let lock = serde_json::to_vec_pretty(&lock).map_err(|e| e.to_string())?;

    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("codec.rs"), source).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("projection.lock"), lock).map_err(|e| e.to_string())?;
    match output.symlink_metadata() {
        Ok(_) => return Err(format!("output already exists: {}", output.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.to_string()),
    }
    fs::rename(stage.path(), output).map_err(|e| e.to_string())
}
