use super::{Binding, Encoding, Graph, Result, absolute_path};
use crate::{
    shared_declarations::unique_map,
    wire_schema::{iri, symbol},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExternalRecord {
    #[serde(deserialize_with = "unique_map")]
    pub(super) fields: BTreeMap<String, Primitive>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Primitive {
    Signed { bits: u8 },
    Unsigned { bits: u8 },
    FixedBytes { size: u32 },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecordBinding {
    pub(super) path: String,
    #[serde(deserialize_with = "unique_map")]
    pub(super) fields: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Integrity {
    pub(super) domain: Vec<u8>,
    pub(super) algorithm: Algorithm,
    pub(super) short: String,
    pub(super) mismatch: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Algorithm {
    Sha256,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum IntegrityBinding {
    Sha2,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Constraint {
    pub(super) id: String,
    pub(super) error: String,
    predicate: Predicate,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Predicate {
    Equal {
        left: Operand,
        right: Operand,
    },
    AtMost {
        left: Operand,
        right: Operand,
    },
    Elapsed {
        end: Operand,
        start: Operand,
        minimum: i64,
    },
    VariantIn {
        field: String,
        variants: BTreeSet<String>,
    },
    Implies {
        condition: Box<Predicate>,
        then: Box<Predicate>,
    },
    Equivalent {
        left: Box<Predicate>,
        right: Box<Predicate>,
    },
    Present {
        field: String,
    },
    OptionalEquals {
        field: String,
        other: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Operand {
    Field { id: String },
    Member { field: String, id: String },
    Unsigned { bits: u8, value: u64 },
    FixedBytes { value: Vec<u8> },
}

impl Primitive {
    pub(super) fn from_encoding(encoding: &Encoding) -> Option<Self> {
        match encoding {
            Encoding::UnsignedBe { bits, .. } => Some(Self::Unsigned { bits: *bits }),
            Encoding::SignedBe { bits } => Some(Self::Signed { bits: *bits }),
            Encoding::FixedBytes { size } => Some(Self::FixedBytes { size: *size }),
            _ => None,
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::Signed { bits } | Self::Unsigned { bits } => matches!(bits, 8 | 16 | 32 | 64),
            Self::FixedBytes { size } => *size > 0,
        }
    }
}

fn field<'a>(graph: &'a Graph, id: &str) -> Option<&'a Encoding> {
    graph
        .layout
        .iter()
        .find(|slot| slot.id == id)
        .map(|slot| &slot.encoding)
}

impl Operand {
    fn kind(&self, graph: &Graph) -> Option<Primitive> {
        match self {
            Self::Field { id } => Primitive::from_encoding(field(graph, id)?),
            Self::Member { field: owner, id } => {
                let Encoding::Nested { record, .. } = field(graph, owner)? else {
                    return None;
                };
                graph.external_records.get(record)?.fields.get(id).copied()
            }
            Self::Unsigned { bits, value }
                if matches!(bits, 8 | 16 | 32 | 64) && *value <= u64::MAX >> (64 - bits) =>
            {
                Some(Primitive::Unsigned { bits: *bits })
            }
            Self::FixedBytes { value } if !value.is_empty() => u32::try_from(value.len())
                .ok()
                .map(|size| Primitive::FixedBytes { size }),
            _ => None,
        }
    }

    fn rust(&self, graph: &Graph, binding: &Binding) -> String {
        match self {
            Self::Field { id } => format!("self.{}", binding.names[id]),
            Self::Member { field: owner, id } => {
                let Some(Encoding::Nested { record, .. }) = field(graph, owner) else {
                    unreachable!("checked member")
                };
                format!(
                    "self.{}.{}",
                    binding.names[owner], binding.external_records[record].fields[id]
                )
            }
            Self::Unsigned { bits, value } => format!("{value}u{bits}"),
            Self::FixedBytes { value } => format!("{value:?}"),
        }
    }
}

impl Predicate {
    fn check(&self, graph: &Graph) -> bool {
        use Primitive::*;
        match self {
            Self::Equal { left, right } => left
                .kind(graph)
                .is_some_and(|ty| Some(ty) == right.kind(graph)),
            Self::AtMost { left, right } => left.kind(graph).is_some_and(|ty| {
                matches!(ty, Signed { .. } | Unsigned { .. }) && Some(ty) == right.kind(graph)
            }),
            Self::Elapsed {
                end,
                start,
                minimum,
            } => {
                *minimum >= 0
                    && end.kind(graph) == Some(Signed { bits: 64 })
                    && start.kind(graph) == Some(Signed { bits: 64 })
            }
            Self::VariantIn {
                field: id,
                variants,
            } => matches!(
                field(graph,id),Some(Encoding::EnumU8 {tags,..}) if !variants.is_empty() && variants.iter().all(|id|tags.contains_key(id))),
            Self::Present { field: id } => {
                matches!(field(graph, id), Some(Encoding::Optional { .. }))
            }
            Self::OptionalEquals { field: id, other } => matches!(
                (field(graph,id),field(graph,other)),
                (Some(Encoding::Optional {payload:super::Payload::Record{id:left},..}),
                 Some(Encoding::Inline {id:right})) if left==right),
            Self::Implies { condition, then } => condition.check(graph) && then.check(graph),
            Self::Equivalent { left, right } => left.check(graph) && right.check(graph),
        }
    }

    fn rust(&self, graph: &Graph, binding: &Binding) -> String {
        let value = |input: &Operand| input.rust(graph, binding);
        let predicate = |input: &Predicate| input.rust(graph, binding);
        match self {
            Self::Equal { left, right } => format!("{} == {}", value(left), value(right)),
            Self::AtMost { left, right } => format!("{} <= {}", value(left), value(right)),
            Self::Elapsed {
                end,
                start,
                minimum,
            } => format!(
                "{}.checked_sub({}).is_some_and(|age| age >= {minimum})",
                value(end),
                value(start)
            ),
            Self::VariantIn {
                field: id,
                variants,
            } => {
                let Some(Encoding::EnumU8 { external, .. }) = field(graph, id) else {
                    unreachable!("validated variants")
                };
                let native = &binding.externals[external];
                let cases = variants
                    .iter()
                    .map(|variant| format!("{}::{}", native.path, native.variants[variant]))
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("matches!(&self.{}, {cases})", binding.names[id])
            }
            Self::Present { field: id } => format!("self.{}.is_some()", binding.names[id]),
            Self::OptionalEquals { field: id, other } => format!(
                "self.{}.as_ref().is_none_or(|value| value == &self.{})",
                binding.names[id], binding.names[other]
            ),
            Self::Implies { condition, then } => {
                format!("!({}) || ({})", predicate(condition), predicate(then))
            }
            Self::Equivalent { left, right } => {
                format!("({}) == ({})", predicate(left), predicate(right))
            }
        }
    }
}

impl Constraint {
    pub(super) fn rust(&self, graph: &Graph, binding: &Binding) -> String {
        self.predicate.rust(graph, binding)
    }
}

pub(super) fn validate<'a>(
    graph: &'a Graph,
    binding: &Binding,
    ids: &mut BTreeSet<&'a str>,
    errors: &mut BTreeSet<&'a str>,
) -> Result<()> {
    let needed: BTreeSet<_> = graph
        .layout
        .iter()
        .filter_map(|slot| match &slot.encoding {
            Encoding::Nested { record, .. } => Some(record),
            _ => None,
        })
        .collect();
    if !needed.iter().copied().eq(graph.external_records.keys())
        || !needed.iter().copied().eq(binding.external_records.keys())
    {
        return Err(
            "nested binary codecs need exact external record declarations and bindings".into(),
        );
    }
    for (id, record) in &graph.external_records {
        iri(id)?;
        let native = &binding.external_records[id];
        if !ids.insert(id)
            || !absolute_path(&native.path)
            || !record.fields.keys().eq(native.fields.keys())
        {
            return Err(format!("invalid nested binary record binding: {id}"));
        }
        let mut names = BTreeSet::new();
        for (id, shape) in &record.fields {
            iri(id)?;
            let name = &native.fields[id];
            if !ids.insert(id) || !shape.valid() || !symbol(name) || !names.insert(name) {
                return Err(format!("invalid nested binary member: {id}"));
            }
        }
    }
    for constraint in &graph.constraints {
        iri(&constraint.id)?;
        if !ids.insert(&constraint.id) || !constraint.predicate.check(graph) {
            return Err(format!(
                "invalid composite record constraint: {}",
                constraint.id
            ));
        }
        errors.insert(&constraint.error);
    }
    match (&graph.integrity, &binding.integrity) {
        (None, None) => {}
        (Some(integrity), Some(_)) => {
            errors.extend([integrity.short.as_str(), integrity.mismatch.as_str()]);
        }
        _ => return Err("binary integrity requires one matching provider hint".into()),
    }
    Ok(())
}
