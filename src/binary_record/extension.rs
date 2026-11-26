use super::{Binding, Encoding, Graph, Result};
use crate::wire_schema::{iri, symbol, type_symbol};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub(super) fields: Vec<Member>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Member {
    pub(super) id: String,
    pub(super) bits: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) signed: bool,
}

impl Member {
    pub(super) fn ty(&self) -> String {
        integer(self.bits, self.signed)
    }
}

pub(super) fn integer(bits: u8, signed: bool) -> String {
    let stem = if signed { "i" } else { "u" };
    format!("::core::primitive::{stem}{bits}")
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Payload {
    UnsignedBe { bits: u8 },
    Record { id: String },
}

impl Payload {
    pub(super) fn width(&self, records: &BTreeMap<String, Record>) -> Result<u64> {
        match self {
            Self::UnsignedBe {
                bits: bits @ (8 | 16 | 32 | 64),
            } => Ok((*bits / 8).into()),
            Self::UnsignedBe { .. } => Err("invalid optional integer width".into()),
            Self::Record { id } => records
                .get(id)
                .ok_or_else(|| format!("unknown optional record: {id}"))
                .map(|record| {
                    record
                        .fields
                        .iter()
                        .map(|field| u64::from(field.bits / 8))
                        .sum()
                }),
        }
    }

    pub(super) fn ty(&self, binding: &Binding) -> String {
        match self {
            Self::UnsignedBe { bits } => integer(*bits, false),
            Self::Record { id } => binding.names[id].clone(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard {
    pub(super) id: String,
    pub(super) error: String,
    condition: Condition,
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Condition {
    PresenceEquals {
        flag: String,
        optional: String,
    },
    OptionalMask {
        optional: String,
        allowed: u64,
        required: u64,
    },
    OptionalMemberRange {
        optional: String,
        member: String,
        minimum: u64,
        maximum: u64,
    },
}

impl Guard {
    fn check(&self, graph: &Graph) -> bool {
        let field = |id: &str| {
            graph
                .layout
                .iter()
                .find(|slot| slot.id == id)
                .map(|slot| &slot.encoding)
        };
        match &self.condition {
            Condition::PresenceEquals { flag, optional } => {
                matches!(field(flag), Some(Encoding::Bool { .. }))
                    && matches!(field(optional), Some(Encoding::Optional { .. }))
            }
            Condition::OptionalMask {
                optional,
                allowed,
                required,
            } => {
                matches!(field(optional), Some(Encoding::Optional { payload: Payload::UnsignedBe { bits }, .. })
                    if *allowed <= u64::MAX >> (64 - bits) && required & !allowed == 0)
            }
            Condition::OptionalMemberRange {
                optional,
                member,
                minimum,
                maximum,
            } => {
                let Some(Encoding::Optional {
                    payload: Payload::Record { id },
                    ..
                }) = field(optional)
                else {
                    return false;
                };
                graph.records[id].fields.iter().any(|item| {
                    item.id == *member
                        && !item.signed
                        && minimum <= maximum
                        && *maximum <= u64::MAX >> (64 - item.bits)
                })
            }
        }
    }

    pub(super) fn expression(&self, binding: &Binding) -> String {
        let name = |id: &str| &binding.names[id];
        match &self.condition {
            Condition::PresenceEquals { flag, optional } => {
                format!("self.{} == self.{}.is_some()", name(flag), name(optional))
            }
            Condition::OptionalMask {
                optional,
                allowed,
                required,
            } => format!(
                "self.{}.is_none_or(|value| value & !{allowed} == 0 && value & {required} == {required})",
                name(optional)
            ),
            Condition::OptionalMemberRange {
                optional,
                member,
                minimum,
                maximum,
            } => format!(
                "self.{}.is_none_or(|value| ({minimum}..={maximum}).contains(&value.{}))",
                name(optional),
                name(member)
            ),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HashView {
    algorithm: Algorithm,
    domain: Vec<u8>,
    fields: Vec<String>,
    prefix: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Algorithm {
    Sha256,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HashBinding {
    method: String,
    library: Library,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Library {
    Sha2,
}

pub(super) fn validate_records<'a>(
    graph: &'a Graph,
    binding: &Binding,
    ids: &mut BTreeSet<&'a str>,
    names: &mut BTreeSet<&'a str>,
) -> Result<()> {
    let mut types = BTreeSet::from([binding.name.as_str()]);
    for (id, record) in &graph.records {
        iri(id)?;
        if !graph.layout.iter().any(|slot| {
            matches!(&slot.encoding, Encoding::Optional { payload: Payload::Record { id: payload }, .. } if payload == id)
        }) {
            return Err(format!("unused inline binary record: {id}"));
        }
        if !ids.insert(id)
            || record.fields.is_empty()
            || !binding
                .names
                .get(id)
                .is_some_and(|name| type_symbol(name) && types.insert(name))
        {
            return Err(format!("invalid inline binary record: {id}"));
        }
        names.insert(id);
        let mut members = BTreeSet::new();
        for field in &record.fields {
            iri(&field.id)?;
            if !ids.insert(&field.id)
                || !matches!(field.bits, 8 | 16 | 32 | 64)
                || !binding.names.get(&field.id).is_some_and(|name| {
                    symbol(name) && name.as_bytes()[0].is_ascii_lowercase() && members.insert(name)
                })
            {
                return Err(format!("invalid inline binary record field: {}", field.id));
            }
            names.insert(&field.id);
        }
    }
    Ok(())
}

pub(super) fn validate_checks<'a>(
    graph: &'a Graph,
    binding: &Binding,
    ids: &mut BTreeSet<&'a str>,
    errors: &mut BTreeSet<&'a str>,
) -> Result<()> {
    for guard in &graph.guards {
        iri(&guard.id)?;
        if !ids.insert(&guard.id) || !guard.check(graph) {
            return Err(format!("invalid binary record guard: {}", guard.id));
        }
        errors.insert(&guard.error);
    }
    if !graph.hash_views.keys().eq(binding.hash_views.keys()) {
        return Err("binary hash views need exact native bindings".into());
    }
    let mut methods = BTreeSet::from([
        "validate",
        "encode",
        "decode",
        "clone",
        "clone_from",
        "eq",
        "ne",
    ]);
    for (id, view) in &graph.hash_views {
        iri(id)?;
        if !ids.insert(id)
            || view.fields.is_empty()
            || !view.fields.iter().all(|field| {
                graph.layout.iter().any(|slot| {
                    slot.id == *field
                        && matches!(
                            slot.encoding,
                            Encoding::FixedBytes { .. } | Encoding::EnumU8 { .. }
                        )
                })
            })
        {
            return Err(format!("invalid binary hash view: {id}"));
        }
        let native = &binding.hash_views[id];
        if !symbol(&native.method)
            || !native
                .method
                .bytes()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'_')
            || !methods.insert(&native.method)
        {
            return Err(format!("invalid binary hash method: {id}"));
        }
    }
    Ok(())
}

pub(super) fn render_records(text: &mut String, graph: &Graph, binding: &Binding) {
    for (id, record) in &graph.records {
        writeln!(
            text,
            "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct {} {{",
            binding.names[id]
        )
        .unwrap();
        for field in &record.fields {
            writeln!(
                text,
                "    pub {}: {},",
                binding.names[&field.id],
                field.ty()
            )
            .unwrap();
        }
        writeln!(text, "}}\n").unwrap();
    }
}

pub(super) fn render_views(text: &mut String, graph: &Graph, binding: &Binding) {
    for (id, view) in &graph.hash_views {
        let native = &binding.hash_views[id];
        writeln!(
            text,
            "    pub fn {}(&self) -> ::std::string::String {{",
            native.method
        )
        .unwrap();
        match (&view.algorithm, &native.library) {
            (Algorithm::Sha256, Library::Sha2) => {
                crate::wire_schema::sha256_begin(text, &view.domain)
            }
        }
        for field in &view.fields {
            let name = &binding.names[field];
            let encoding = &graph
                .layout
                .iter()
                .find(|slot| slot.id == *field)
                .expect("validated slot")
                .encoding;
            match encoding {
                Encoding::FixedBytes { .. } => {
                    writeln!(text, "        __oddlang_hash.update(self.{name});").unwrap()
                }
                Encoding::EnumU8 { external, tags, .. } => {
                    let native = &binding.externals[external];
                    crate::wire_schema::sha256_enum(
                        text,
                        &format!("self.{name}"),
                        &native.path,
                        tags,
                        &native.variants,
                    );
                }
                _ => unreachable!("validated slot encoding"),
            }
        }
        crate::wire_schema::sha256_hex(text, &view.prefix);
        writeln!(text, "        __oddlang_text\n    }}").unwrap();
    }
}
