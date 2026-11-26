use super::{EndpointView, Native, Protocol, Source, View, digests, preflight, rules::Compiler};
use crate::wire_schema::{
    Binding, Payload, Result, Schema, Shape,
    render::{expression, ty},
};
use std::{collections::BTreeMap, fmt::Write as _};

pub(super) const ERROR: &str = "__OddlangProtocolError";

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
    text.push_str(&compiler.record_checks()?);
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
    let bounds = preflight::Bounds::new(schema);
    text.push_str(&preflight::runtime(
        limit,
        &size,
        &admission,
        view.flat.is_some(),
    ));
    text.push_str(&bounds.emit(schema, binding));
    let (decode, encode) = if let Some((selector, tag, content)) = view.flat {
        text.push_str(&flat_adapter(
            env_type, selector, tag, content, &size, &admission,
        ));
        (
            "__oddlang_from_flat(bytes)?".to_owned(),
            "__oddlang_to_flat",
        )
    } else {
        (
            format!("::serde_json::from_slice(bytes).map_err(|_| {admission})?"),
            "__oddlang_serialize_frame",
        )
    };
    writeln!(
        text,
        r#"
fn __oddlang_decode_frame(bytes: &[u8]) -> ::core::result::Result<{env_type}, {ERROR}> {{
    if bytes.len() > {limit} {{
        return ::core::result::Result::Err({size});
    }}
    let value: {env_type} = {decode};"#
    )
    .unwrap();
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
        r#"    if {encode}(&value)?.as_slice() != bytes {{
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
            &mut text, index, graph, native, binding, view, endpoint, &steps, &timed, &bounds,
        );
    }
    digests::emit(&mut text, &mut compiler, view, &bounds)?;
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
    bounds: &preflight::Bounds<'_>,
) {
    let name = &endpoint.target.ty;
    let encode_envelope = if view.flat.is_some() {
        "__oddlang_to_flat"
    } else {
        "__oddlang_serialize_frame"
    };
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
{encode_validation}"#
    )
    .unwrap();
    preflight::check_fields(text, bounds, &endpoint.fields, |field| {
        format!("&value.{}", field.name)
    });
    writeln!(text, "    let envelope = {env} {{").unwrap();
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
    {encode_envelope}(&envelope)
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

fn flat_adapter(
    env: &str,
    selector: &str,
    tag: &str,
    content: &str,
    size: &str,
    admission: &str,
) -> String {
    // The intermediate nested representation adds a quoted name, colon and two braces.
    let overhead = serde_json::to_vec(selector)
        .expect("string serializes")
        .len()
        + 3;
    format!(
        r#"
fn __oddlang_to_flat(value: &{env}) -> ::core::result::Result<::std::vec::Vec<u8>, {ERROR}> {{
    let mut writer = __OddlangWireBytes {{
        limit: __ODDLANG_MAX_WIRE_BYTES.saturating_add({overhead}),
        bytes: ::std::vec::Vec::new(),
        overflow: false,
    }};
    let status = ::serde_json::to_writer(&mut writer, value);
    if writer.overflow {{ return ::core::result::Result::Err({size}); }}
    status.map_err(|_| {admission})?;
    let mut frame: ::serde_json::Value = ::serde_json::from_slice(&writer.bytes).map_err(|_| {admission})?;
    let fields = frame.as_object_mut().ok_or_else(|| {admission})?;
    let choice = fields.remove({selector:?}).ok_or_else(|| {admission})?;
    let ::serde_json::Value::Object(entries) = choice else {{ return ::core::result::Result::Err({admission}); }};
    fields.extend(entries);
    frame.sort_all_objects();
    __oddlang_serialize_frame(&frame)
}}

fn __oddlang_from_flat(bytes: &[u8]) -> ::core::result::Result<{env}, {ERROR}> {{
    let mut frame: ::serde_json::Value = ::serde_json::from_slice(bytes).map_err(|_| {admission})?;
    let fields = frame.as_object_mut().ok_or_else(|| {admission})?;
    if fields.contains_key({selector:?}) {{ return ::core::result::Result::Err({admission}); }}
    let mut choice = ::serde_json::Map::new();
    choice.insert({tag:?}.into(), fields.remove({tag:?}).ok_or_else(|| {admission})?);
    if let Some(value) = fields.remove({content:?}) {{ choice.insert({content:?}.into(), value); }}
    fields.insert({selector:?}.into(), ::serde_json::Value::Object(choice));
    ::serde_json::from_value(frame).map_err(|_| {admission})
}}
"#
    )
}
