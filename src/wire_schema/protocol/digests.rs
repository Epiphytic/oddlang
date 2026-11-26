//! SHA-256 and bounded fixed-byte native helper bindings.
use super::{
    View, preflight,
    rules::{Compiler, Step},
};
use crate::wire_schema::{
    Definition, Field, Payload, Result, Shape, iri, render::ty, symbol, type_symbol,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt::Write as _};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Alias {
    pub(super) id: String,
    pub(super) bytes: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HexExport {
    pub id: String,
    bytes: usize,
    diagnostic: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Digest {
    Bytes {
        id: String,
    },
    Case {
        id: String,
        union: String,
        variant: String,
        parameters: Vec<Parameter>,
        steps: Vec<Step>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameter {
    id: String,
    field: String,
    passing: Passing,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Passing {
    Owned,
    BorrowedList,
}

impl Digest {
    pub fn id(&self) -> &str {
        match self {
            Self::Bytes { id } | Self::Case { id, .. } => id,
        }
    }
    pub fn declarations(&self) -> Vec<&str> {
        let mut ids = vec![self.id()];
        if let Self::Case {
            parameters, steps, ..
        } = self
        {
            ids.extend(parameters.iter().map(|parameter| parameter.id.as_str()));
            ids.extend(steps.iter().flat_map(Step::declarations));
        }
        ids
    }

    pub fn errors<'a>(&'a self, errors: &mut BTreeSet<&'a str>) {
        if let Self::Case { steps, .. } = self {
            for step in steps {
                step.errors(errors);
            }
        }
    }
}

pub(super) fn emit(
    text: &mut String,
    compiler: &mut Compiler<'_>,
    view: &View<'_>,
    bounds: &preflight::Bounds<'_>,
) -> Result<()> {
    let graph = compiler.graph;
    let native = compiler.native;
    let binding = compiler.binding;
    let schema = compiler.schema;
    let mut ids = BTreeSet::new();
    let mut globals: BTreeSet<_> = schema
        .types
        .iter()
        .filter_map(|record| binding.names.get(record.id()).map(String::as_str))
        .collect();
    globals.extend(native.constants.values().map(String::as_str));
    for entry in &view.endpoints {
        globals.extend([
            entry.target.ty.as_str(),
            entry.target.encode.as_str(),
            entry.target.decode.as_str(),
        ]);
    }
    let name = |id: &str| -> Result<&str> {
        iri(id)?;
        let name = native
            .auxiliary
            .get(id)
            .ok_or("unbound native auxiliary operation")?;
        if !symbol(name) {
            return Err("invalid native auxiliary binding".into());
        }
        Ok(name.as_str())
    };
    for alias in &graph.aliases {
        let native = name(&alias.id)?;
        if !(1..=4096).contains(&alias.bytes)
            || !type_symbol(native)
            || !globals.insert(native)
            || !ids.insert(alias.id.as_str())
        {
            return Err("invalid fixed-byte alias".into());
        }
        writeln!(text, "pub type {native} = [u8; {}];", alias.bytes).unwrap();
    }
    for adapter in &graph.hex_exports {
        let native = name(&adapter.id)?;
        if !(1..=4096).contains(&adapter.bytes)
            || !globals.insert(native)
            || !ids.insert(adapter.id.as_str())
        {
            return Err("invalid fixed hex adapter export".into());
        }
        let bytes = adapter.bytes;
        let diagnostic = &adapter.diagnostic;
        writeln!(text, r#"
#[allow(dead_code)]
pub(crate) mod {native} {{
    pub fn serialize<S: ::serde::Serializer>(value: &[u8; {bytes}], writer: S) -> Result<S::Ok, S::Error> {{
        super::__oddlang_wire_hex::serialize(value, writer)
    }}
    pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(reader: D) -> Result<[u8; {bytes}], D::Error> {{
        super::__oddlang_wire_hex::deserialize(reader, {diagnostic:?})
    }}
}}"#).unwrap();
    }
    if !graph.digests.is_empty() {
        text.push_str(
            r#"
fn __oddlang_protocol_sha256(bytes: &[u8]) -> [u8;32] {
    use ::sha2::Digest as _;
    ::sha2::Sha256::digest(bytes).into()
}
"#,
        );
    }
    for digest in &graph.digests {
        let function = name(digest.id())?;
        if !ids.insert(digest.id()) || !globals.insert(function) {
            return Err("duplicate digest operation".into());
        }
        match digest {
            Digest::Bytes { .. } => writeln!(
                text,
                r#"
pub fn {function}(bytes: &[u8]) -> [u8;32] {{
    __oddlang_protocol_sha256(bytes)
}}"#
            )
            .unwrap(),
            Digest::Case {
                union,
                variant,
                parameters,
                steps,
                ..
            } => {
                let Some(Definition::Adjacent { variants, .. }) =
                    schema.types.iter().find(|item| item.id() == union)
                else {
                    return Err("serialized digest must name an adjacent type".into());
                };
                let selected = variants
                    .iter()
                    .find(|item| item.id == *variant)
                    .ok_or("unknown digested case")?;
                let Payload::Record { fields } = &selected.payload else {
                    return Err("serialized digest needs a named case payload".into());
                };
                let mut consumed = BTreeSet::new();
                let mut parameters_used = BTreeSet::new();
                let mut args = Vec::new();
                let mut mapped = Vec::new();
                let mut construction = Vec::new();
                for parameter in parameters {
                    let argument = name(&parameter.id)?;
                    if !parameters_used.insert(argument)
                        || !ids.insert(parameter.id.as_str())
                        || !consumed.insert(&parameter.field)
                    {
                        return Err("duplicate digest parameter binding".into());
                    }
                    let field = fields
                        .iter()
                        .find(|field| field.id == parameter.field)
                        .ok_or("unknown digest case field")?;
                    if !native.shape_is_public(&field.shape, graph, view.union) {
                        return Err("public digest argument refers to the internal frame".into());
                    }
                    let (parameter_type, expression) = match parameter.passing {
                        Passing::Owned => (ty(&field.shape, binding), argument.to_owned()),
                        Passing::BorrowedList => {
                            let item = match &field.shape {
                                Shape::List { item }
                                    if crate::wire_schema::supports_derive(
                                        item, "Clone", binding,
                                    ) =>
                                {
                                    ty(item, binding)
                                }
                                Shape::BoundedBase64List { .. } => "::std::vec::Vec<u8>".to_owned(),
                                _ => {
                                    return Err(
                                        "borrowed-list digest parameter requires a cloneable ordered list"
                                            .into(),
                                    );
                                }
                            };
                            (format!("&[{item}]"), format!("{argument}.to_vec()"))
                        }
                    };
                    args.push(format!("{argument}: {parameter_type}"));
                    mapped.push(Field {
                        id: parameter.id.clone(),
                        name: argument.to_owned(),
                        shape: field.shape.clone(),
                    });
                    construction.push(format!("{}: {expression}", binding.names[&field.id]));
                }
                if consumed.len() != fields.len() {
                    return Err("serialized digest must bind every case field".into());
                }
                let args = args.join(", ");
                let constructed = construction.join(", ");
                let role = &binding.names[union];
                let case = &binding.names[variant];
                writeln!(
                    text,
                    "
pub fn {function}({args}) -> ::core::result::Result<[u8;32], __OddlangProtocolError> {{"
                )
                .unwrap();
                text.push_str(&compiler.digest(&mapped, steps)?);
                preflight::check_fields(text, bounds, &mapped, |field| format!("&{}", field.name));
                writeln!(
                    text,
                    r#"    let case = {role}::{case} {{ {constructed} }};
    let bytes = __oddlang_serialize_frame(&case)?;
    ::core::result::Result::Ok(__oddlang_protocol_sha256(&bytes))
}}"#
                )
                .unwrap();
            }
        }
    }
    if ids
        .iter()
        .copied()
        .ne(native.auxiliary.keys().map(String::as_str))
    {
        return Err("native auxiliary binding must cover each declared identity once".into());
    }
    Ok(())
}
