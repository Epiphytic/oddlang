use super::wire::{Contract, Errors, Field, Range, Shape, StreamErrors, Sum, Variant, width_max};
use crate::rust_recovery::tagged::FieldObservation;
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{
        GraphName, Literal, NamedNode, NamedOrBlankNode, Term, Triple,
        vocab::{rdf, xsd},
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const NS: &str = "urn:oddlang:tagged-wire:v1:";
type Result<T> = std::result::Result<T, String>;

pub fn load(path: &Path) -> Result<Contract> {
    decode(&fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?)
}

pub(super) fn complete_tag(
    overlay: &Path,
    variant: &str,
    name: &str,
    tag: u8,
    fields: &[FieldObservation],
) -> Result<Vec<u8>> {
    if !valid_id(variant) {
        return Err("invalid selected tagged wire variant IRI".into());
    }
    let subject = NamedNode::new(variant).map_err(|err| err.to_string())?;
    let bytes = fs::read(overlay).map_err(|err| format!("{}: {err}", overlay.display()))?;
    let property = |name: &str| NamedNode::new_unchecked(format!("{NS}{name}"));
    let mut lines = BTreeSet::new();
    let mut matched = false;
    for row in RdfParser::from_format(RdfFormat::Turtle).for_slice(&bytes) {
        let row = row.map_err(|err| format!("tagged wire overlay: {err}"))?;
        if matches!(&row.subject, NamedOrBlankNode::NamedNode(node) if node == &subject) {
            if row.predicate == property("tag") {
                return Err("reviewed overlay already supplies the recovered variant tag".into());
            }
            if row.predicate == property("name") && row.object == Term::Literal(Literal::from(name))
            {
                matched = true;
            }
        }
        lines.insert(row.to_string());
    }
    if !matched {
        return Err("reviewed overlay variant name differs from Rust observation".into());
    }
    lines.insert(
        Triple::new(
            subject,
            property("tag"),
            Literal::new_typed_literal(tag.to_string(), xsd::INTEGER),
        )
        .to_string(),
    );
    let mut completed = Vec::new();
    for line in lines {
        completed.extend_from_slice(line.as_bytes());
        completed.extend_from_slice(b" .\n");
    }
    let contract = decode(&completed)?;
    let observed = &contract.variants[&tag];
    if observed.fields.len() != fields.len() {
        return Err("reviewed overlay field count differs from Rust observations".into());
    }
    for (position, (field, observation)) in observed.fields.iter().zip(fields).enumerate() {
        let shape_matches = match (&field.shape, observation.kind) {
            (Shape::UInt { width, .. }, "uint-be") => Some(*width) == observation.width,
            (Shape::RestBytes { .. }, "rest-bytes") => observation.width.is_none(),
            _ => false,
        };
        if observation.position != position || observation.name != field.name || !shape_matches {
            return Err(format!(
                "reviewed overlay field {position} differs from Rust observations"
            ));
        }
    }
    Ok(completed)
}

fn decode(bytes: &[u8]) -> Result<Contract> {
    let mut graph = Graph::default();
    let mut lines = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for line in RdfParser::from_format(RdfFormat::Turtle).for_slice(bytes) {
        let row = line.map_err(|err| format!("tagged wire RDF: {err}"))?;
        lines.insert(row.to_string());
        let NamedOrBlankNode::NamedNode(subject) = row.subject else {
            return Err("tagged wire subjects must be named".into());
        };
        let subject = subject.into_string();
        if !valid_id(&subject) || row.graph_name != GraphName::DefaultGraph {
            return Err(format!("invalid tagged wire subject {subject}"));
        }
        let predicate = row.predicate.as_str();
        if predicate == rdf::TYPE.as_str() {
            if matches!(row.object, Term::NamedNode(ref kind) if kind.as_str() == format!("{NS}Contract"))
            {
                roots.insert(subject);
                continue;
            }
            return Err("invalid tagged wire RDF type".into());
        }
        let property = predicate
            .strip_prefix(NS)
            .ok_or_else(|| format!("unknown tagged wire predicate {predicate}"))?;
        let values = graph
            .rows
            .entry((subject, property.to_owned()))
            .or_default();
        if !values.contains(&row.object) {
            values.push(row.object);
        }
    }
    let [root] = roots
        .into_iter()
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "expected exactly one tagged wire contract")?;
    let max_payload = graph
        .number::<u32>(&root, "maxPayload")?
        .filter(|n| *n > 0)
        .ok_or("invalid maxPayload")?;
    let errors = Errors {
        short_header: graph.error(&root, "shortHeader")?,
        payload_limit: graph.error(&root, "payloadLimit")?,
        length_mismatch: graph.error(&root, "lengthMismatch")?,
        unknown_tag: graph.error(&root, "unknownTag")?,
        truncated: graph.error(&root, "truncated")?,
        trailing: graph.error(&root, "trailing")?,
        invalid_value: graph.error(&root, "invalidValue")?,
    };
    let stream = graph
        .one(&root, "feedFailed")?
        .map(|link| {
            Ok::<_, String>(StreamErrors {
                failed: graph.error_code(Graph::link(link)?)?,
                incomplete: graph.error(&root, "incomplete")?,
            })
        })
        .transpose()?;
    let mut variants = BTreeMap::new();
    let mut names = BTreeSet::new();
    for variant in graph.links(&root, "variant")? {
        let tag = graph
            .number(&variant, "tag")?
            .ok_or("missing variant tag")?;
        let name = graph.name(&variant)?;
        let fields = graph.fields(&variant, max_payload)?;
        if !names.insert(name.clone()) || variants.insert(tag, Variant { name, fields }).is_some() {
            return Err("duplicate variant name or tag".into());
        }
    }
    if variants.is_empty() {
        return Err("a tagged wire contract needs at least one variant".into());
    }
    if let Some(((node, field), _)) = graph.rows.first_key_value() {
        return Err(format!("unexpected tagged wire field {field} on {node}"));
    }
    let mut digest = Sha256::new();
    for line in lines {
        digest.update(line.as_bytes());
        digest.update(b" .\n");
    }
    let sha256 = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Contract {
        max_payload,
        errors,
        stream,
        variants,
        sha256,
    })
}

#[derive(Default)]
struct Graph {
    rows: BTreeMap<(String, String), Vec<Term>>,
    errors: BTreeMap<String, String>,
}

impl Graph {
    fn values(&mut self, node: &str, field: &str) -> Vec<Term> {
        self.rows
            .remove(&(node.to_owned(), field.to_owned()))
            .unwrap_or_default()
    }

    fn one(&mut self, node: &str, field: &str) -> Result<Option<Term>> {
        let mut values = self.values(node, field);
        if values.len() > 1 {
            return Err(format!("expected one {field} on {node}"));
        }
        Ok(values.pop())
    }

    fn number<T: std::str::FromStr + std::fmt::Display>(
        &mut self,
        node: &str,
        field: &str,
    ) -> Result<Option<T>> {
        self.one(node, field)?
            .map(|value| match value {
                Term::Literal(value) if value.datatype() == xsd::INTEGER => value
                    .value()
                    .parse::<T>()
                    .ok()
                    .filter(|integer| integer.to_string() == value.value())
                    .ok_or_else(|| format!("invalid or noncanonical integer {field} on {node}")),
                _ => Err(format!("expected integer {field} on {node}")),
            })
            .transpose()
    }

    fn text(&mut self, node: &str, field: &str) -> Result<String> {
        match self.one(node, field)? {
            Some(Term::Literal(value)) if value.datatype() == xsd::STRING => {
                Ok(value.value().to_owned())
            }
            _ => Err(format!("expected string {field} on {node}")),
        }
    }

    fn name(&mut self, node: &str) -> Result<String> {
        let name = self.text(node, "name")?;
        if !label(&name) {
            return Err(format!("invalid name on {node}"));
        }
        Ok(name)
    }

    fn link(value: Term) -> Result<String> {
        match value {
            Term::NamedNode(iri) if valid_id(iri.as_str()) => Ok(iri.into_string()),
            _ => Err("expected a named tagged wire identifier".into()),
        }
    }

    fn links(&mut self, node: &str, field: &str) -> Result<Vec<String>> {
        self.values(node, field)
            .into_iter()
            .map(Self::link)
            .collect()
    }

    fn error(&mut self, node: &str, field: &str) -> Result<String> {
        let iri = Self::link(
            self.one(node, field)?
                .ok_or_else(|| format!("missing {field} on {node}"))?,
        )?;
        self.error_code(iri)
    }

    fn error_code(&mut self, iri: String) -> Result<String> {
        if let Some(code) = self.errors.get(&iri) {
            return Ok(code.clone());
        }
        let code = self.text(&iri, "code")?;
        if code.len() > 129 || code.split('/').count() > 2 || !code.split('/').all(label) {
            return Err(format!("invalid error code on {iri}"));
        }
        self.errors.insert(iri, code.clone());
        Ok(code)
    }

    fn fields(&mut self, variant: &str, max_payload: u32) -> Result<Vec<Field>> {
        let mut fields = BTreeMap::new();
        let mut names = BTreeSet::new();
        for id in self.links(variant, "field")? {
            let position = self
                .number::<u32>(&id, "position")?
                .ok_or("missing field position")?;
            let name = self.name(&id)?;
            if !names.insert(name.clone()) || fields.insert(position, (id, name)).is_some() {
                return Err(format!("duplicate field name or position on {variant}"));
            }
        }
        let total = fields.len();
        let mut result: Vec<Field> = Vec::new();
        let mut indexes = BTreeMap::<String, usize>::new();
        for (position, (id, name)) in fields {
            if position as usize != result.len() {
                return Err(format!("nonconsecutive field positions on {variant}"));
            }
            let shape = match self.text(&id, "kind")?.as_str() {
                "uint-be" => {
                    let width = self
                        .number::<u8>(&id, "width")?
                        .filter(|value| matches!(value, 1 | 2 | 4 | 8))
                        .ok_or("invalid integer width")?;
                    let range = self
                        .number::<u64>(&id, "max")?
                        .map(|max| {
                            if max > width_max(width) {
                                return Err("integer maximum exceeds its wire width".to_owned());
                            }
                            Ok(Range {
                                max,
                                error: self.error(&id, "rangeError")?,
                            })
                        })
                        .transpose()?;
                    Shape::UInt { width, range }
                }
                "fixed-bytes" => {
                    let width = self
                        .number::<u32>(&id, "width")?
                        .filter(|width| *width <= max_payload)
                        .ok_or("invalid fixed field width")?;
                    Shape::FixedBytes { width }
                }
                "rest-bytes" => {
                    if position as usize + 1 != total {
                        return Err("rest-bytes field must be last".into());
                    }
                    let min = self
                        .number::<u32>(&id, "min")?
                        .ok_or("missing rest-bytes min")?;
                    let max = self
                        .number::<u32>(&id, "max")?
                        .filter(|max| min <= *max && *max <= max_payload)
                        .ok_or("invalid rest-bytes max")?;
                    let error = self.error(&id, "rangeError")?;
                    let sum = self
                        .one(&id, "sumWith")?
                        .map(|target| {
                            let target = Self::link(target)?;
                            let field = *indexes
                                .get(&target)
                                .ok_or("sumWith must refer to an earlier field")?;
                            let Shape::UInt {
                                range: Some(range), ..
                            } = &result[field].shape
                            else {
                                return Err("sumWith requires a ranged integer".to_owned());
                            };
                            Ok(Sum {
                                field,
                                max: range.max,
                                error: self.error(&id, "sumError")?,
                            })
                        })
                        .transpose()?;
                    Shape::RestBytes {
                        min,
                        max,
                        error,
                        sum,
                    }
                }
                kind => return Err(format!("unsupported field kind {kind}")),
            };
            indexes.insert(id, result.len());
            result.push(Field { name, shape });
        }
        Ok(result)
    }
}

fn label(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

fn valid_id(iri: &str) -> bool {
    iri.starts_with("urn:")
        && iri.len() <= 256
        && iri
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:./#".contains(&byte))
}
