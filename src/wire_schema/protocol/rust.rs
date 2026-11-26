use super::{EndpointView, Native, Protocol, Source, View, digests, rules::Compiler};
use crate::wire_schema::{
    Binding, Payload, Result, Schema, Shape,
    render::{expression, ty},
};
use std::{collections::BTreeMap, fmt::Write as _};

const ERROR: &str = "__OddlangProtocolError";

pub(super) fn error(graph: &Protocol, native: &Native, id: &str) -> String {
    let constructor = &graph.errors[id];
    let variant = &native.errors[&constructor.variant];
    match &constructor.argument {
        Some(arg) => format!("{ERROR}::{variant}({arg:?})"),
        None => format!("{ERROR}::{variant}"),
    }
}

pub(super) fn render(
    graph: &Protocol,
    native: &Native,
    schema: &Schema,
    binding: &Binding,
    view: &View<'_>,
) -> Result<String> {
    let mut compiler = Compiler::new(graph, schema, binding, native);
    compiler.validate_foreign()?;
    let mut text = format!("\ntype {ERROR} = {};\n", native.error_type);
    for item in &graph.constants {
        let (kind, value) = match item.shape {
            Shape::Text {} => ("&str".into(), format!("{:?}", item.value.as_str().unwrap())),
            _ => (ty(&item.shape, binding), item.value.to_string()),
        };
        writeln!(
            text,
            "pub const {}: {kind} = {value};",
            native.constants[&item.id]
        )
        .unwrap();
    }
    let env_type = &binding.names[&graph.envelope];
    let size = error(graph, native, &graph.size_error);
    let admission = error(graph, native, &graph.admission_error);
    let limit = graph.maximum_wire_bytes;
    writeln!(text, r#"
fn __oddlang_encode_frame(value: &{env_type}) -> ::core::result::Result<::std::vec::Vec<u8>, {ERROR}> {{
    let bytes = ::serde_json::to_vec(value).map_err(|_| {admission})?;
    if bytes.len() > {limit} {{
        return ::core::result::Result::Err({size});
    }}
    ::core::result::Result::Ok(bytes)
}}

fn __oddlang_decode_frame(bytes: &[u8]) -> ::core::result::Result<{env_type}, {ERROR}> {{
    if bytes.len() > {limit} {{
        return ::core::result::Result::Err({size});
    }}
    let value: {env_type} = ::serde_json::from_slice(bytes).map_err(|_| {admission})?;"#).unwrap();
    for (field, id) in &graph.literals {
        writeln!(
            text,
            r#"    if value.{} != {} {{
        return ::core::result::Result::Err({admission});
    }}"#,
            binding.names[field], native.constants[id]
        )
        .unwrap();
    }
    writeln!(
        text,
        r#"    if __oddlang_encode_frame(&value)?.as_slice() != bytes {{
        return ::core::result::Result::Err({admission});
    }}
    ::core::result::Result::Ok(value)
}}"#
    )
    .unwrap();
    for (index, endpoint) in view.endpoints.iter().enumerate() {
        let steps = compiler.endpoint(endpoint, &endpoint.graph.steps, false)?;
        let timed = compiler.endpoint(endpoint, &endpoint.graph.timed, true)?;
        emit_endpoint(
            &mut text, index, graph, native, binding, view, endpoint, &steps, &timed,
        );
    }
    digests::emit(&mut text, &mut compiler, view)?;
    Ok(text)
}

fn emit_endpoint(
    text: &mut String,
    index: usize,
    graph: &Protocol,
    native: &Native,
    binding: &Binding,
    view: &View<'_>,
    endpoint: &EndpointView<'_>,
    steps: &str,
    timed: &str,
) {
    let name = &endpoint.target.ty;
    let encode = &endpoint.target.encode;
    let decode = &endpoint.target.decode;
    let env = &binding.names[&graph.envelope];
    let choice = &binding.names[view.union];
    let variant = &binding.names[&endpoint.variant.id];
    let selector = &binding.names[&graph.selector];
    let admission = error(graph, native, &graph.admission_error);
    if !endpoint.target.derives.is_empty() {
        let derives = endpoint
            .target
            .derives
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(text, "\n#[derive({derives})]").unwrap();
    }
    writeln!(text, "pub struct {name} {{").unwrap();
    for item in &endpoint.fields {
        writeln!(text, "    pub {}: {},", item.name, ty(&item.shape, binding)).unwrap();
    }
    text.push_str("}\n");

    let validate = format!("__oddlang_validate_{index}");
    let has_admission = !endpoint.graph.guards.is_empty() || !steps.is_empty();
    if has_admission {
        writeln!(
            text,
            "\nfn {validate}(value: &{name}) -> ::core::result::Result<(), {ERROR}> {{"
        )
        .unwrap();
        let operands: BTreeMap<_, _> = endpoint
            .fields
            .iter()
            .map(|item| (item.id.as_str(), (format!("value.{}", item.name), false)))
            .collect();
        for guard in &endpoint.graph.guards {
            let assertion = expression(&guard.require, &operands);
            let failure = error(graph, native, &guard.error);
            let when = guard.when.as_ref().map_or_else(String::new, |predicate| {
                format!("{} && ", expression(predicate, &operands))
            });
            writeln!(text, "    if {when}!({assertion}) {{\n        return ::core::result::Result::Err({failure});\n    }}").unwrap();
        }
        text.push_str(steps);
        text.push_str("    ::core::result::Result::Ok(())\n}\n");
    }
    if let Some(method) = &endpoint.target.timed {
        writeln!(text, "\nimpl {name} {{\n    pub fn {method}(&self, now: u64) -> ::core::result::Result<(), {ERROR}> {{\n").unwrap();
        if has_admission {
            writeln!(text, "        {validate}(self)?;").unwrap();
        }
        for line in timed.lines() {
            writeln!(text, "    {line}").unwrap();
        }
        text.push_str("        ::core::result::Result::Ok(())\n    }\n}\n");
    }
    let encode_validation = if has_admission {
        format!("    {validate}(value)?;\n")
    } else {
        String::new()
    };
    let validation = if has_admission {
        format!("    {validate}(&value)?;\n")
    } else {
        String::new()
    };
    writeln!(
        text,
        r#"
pub fn {encode}(value: &{name}) -> ::core::result::Result<::std::vec::Vec<u8>, {ERROR}> {{
{encode_validation}    let envelope = {env} {{"#
    )
    .unwrap();
    let get = |source: &Source| {
        let index = endpoint
            .graph
            .fields
            .iter()
            .position(|mapped| &mapped.from == source)
            .expect("validated mapping");
        &endpoint.fields[index].name
    };
    for field in view.envelope {
        let rust = &binding.names[&field.id];
        if field.id == graph.selector {
            match &endpoint.variant.payload {
                Payload::Unit {} => writeln!(text, "        {rust}: {choice}::{variant},").unwrap(),
                Payload::Newtype { .. } => {
                    let name = get(&Source::CaseValue {});
                    writeln!(
                        text,
                        "        {rust}: {choice}::{variant}(value.{name}.clone()),"
                    )
                    .unwrap();
                }
                Payload::Record { fields } => {
                    writeln!(text, "        {rust}: {choice}::{variant} {{").unwrap();
                    for field in fields {
                        let name = get(&Source::CaseField {
                            field: field.id.clone(),
                        });
                        writeln!(
                            text,
                            "            {}: value.{name}.clone(),",
                            binding.names[&field.id]
                        )
                        .unwrap();
                    }
                    text.push_str("        },\n");
                }
            }
        } else if let Some(id) = graph.literals.get(&field.id) {
            let constant = &native.constants[id];
            let owned = if matches!(field.shape, Shape::Text {}) {
                ".to_owned()"
            } else {
                ""
            };
            writeln!(text, "        {rust}: {constant}{owned},").unwrap();
        } else {
            let name = get(&Source::EnvelopeField {
                field: field.id.clone(),
            });
            writeln!(text, "        {rust}: value.{name}.clone(),").unwrap();
        }
    }
    writeln!(
        text,
        r#"    }};
    __oddlang_encode_frame(&envelope)
}}

pub fn {decode}(bytes: &[u8]) -> ::core::result::Result<{name}, {ERROR}> {{
    let envelope = __oddlang_decode_frame(bytes)?;"#
    )
    .unwrap();
    let mut sources = BTreeMap::new();
    write!(text, "    let {choice}::{variant}").unwrap();
    match &endpoint.variant.payload {
        Payload::Unit {} => {}
        Payload::Newtype { .. } => {
            text.push_str("(__oddlang_case)");
            sources.insert(Source::CaseValue {}, "__oddlang_case".to_owned());
        }
        Payload::Record { fields } => {
            text.push_str(" { ");
            for (pos, field) in fields.iter().enumerate() {
                let name = format!("__oddlang_case_{pos}");
                write!(text, "{}: {name}, ", binding.names[&field.id]).unwrap();
                sources.insert(
                    Source::CaseField {
                        field: field.id.clone(),
                    },
                    name,
                );
            }
            text.push('}');
        }
    }
    write!(text, " = envelope.{selector}").unwrap();
    if view.union_cases > 1 {
        write!(
            text,
            " else {{\n        return ::core::result::Result::Err({admission});\n    }}"
        )
        .unwrap();
    }
    writeln!(text, ";\n    let value = {name} {{").unwrap();
    for (mapped, item) in endpoint.graph.fields.iter().zip(&endpoint.fields) {
        let name = &item.name;
        if let Source::EnvelopeField { field } = &mapped.from {
            writeln!(text, "        {name}: envelope.{},", binding.names[field]).unwrap();
        } else {
            writeln!(text, "        {name}: {},", sources[&mapped.from]).unwrap();
        }
    }
    writeln!(
        text,
        "    }};\n{validation}    ::core::result::Result::Ok(value)\n}}"
    )
    .unwrap();
}
