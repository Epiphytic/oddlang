use super::*;
use std::fmt::Write as _;

pub(super) fn literal(value: &Literal) -> String {
    match value {
        Literal::U8(value) => value.to_string(),
        Literal::Text(value) => format!("{value:?}"),
    }
}

fn derives(out: &mut String, target: &Native) {
    let copy = if target.traits.contains(&Derived::Copy) {
        ", Copy"
    } else {
        ""
    };
    let hash = if target.traits.contains(&Derived::Hash) {
        ", Hash"
    } else {
        ""
    };
    writeln!(out, "#[derive(Clone{copy}, Debug, PartialEq, Eq{hash})]").unwrap();
}

fn scalar_type(value: &Literal) -> (&'static str, &'static str) {
    match value {
        Literal::U8(_) => ("::core::primitive::u8", "::core::primitive::u8"),
        Literal::Text(_) => ("&'static ::core::primitive::str", "&::core::primitive::str"),
    }
}

fn enumeration(
    out: &mut String,
    values: &BTreeMap<String, Literal>,
    failure: Option<&str>,
    binding: &Binding,
    target: &Native,
) {
    let name = &target.name;
    writeln!(out, "pub enum {name} {{").unwrap();
    for id in &target.order {
        writeln!(out, "    {},", binding.variants[id]).unwrap();
    }
    let (output, arg) = scalar_type(values.values().next().expect("validated enum"));
    let view = target.view.as_ref().expect("validated enum view");
    let receiver = if target.traits.contains(&Derived::Copy) {
        "self"
    } else {
        "&self"
    };
    writeln!(
        out,
        "}}\nimpl {name} {{\n    pub fn {view}({receiver}) -> {output} {{\n        match self {{"
    )
    .unwrap();
    for id in &target.order {
        writeln!(
            out,
            "            Self::{} => {},",
            binding.variants[id],
            literal(&values[id])
        )
        .unwrap();
    }
    out.push_str("        }\n    }\n");
    if let Some(failure) = failure {
        let fault = &binding.errors[failure];
        let parse = target.parse.as_ref().expect("validated string parser");
        let error = &fault.error_type;
        writeln!(out,"    pub fn {parse}(value: {arg}) -> ::core::result::Result<Self, {error}> {{\n        match value {{").unwrap();
        for id in &target.order {
            writeln!(
                out,
                "            {} => ::core::result::Result::Ok(Self::{}),",
                literal(&values[id]),
                binding.variants[id]
            )
            .unwrap();
        }
        if !matches!(values.values().next(), Some(Literal::U8(_))) || values.len() < 256 {
            writeln!(
                out,
                "            _ => ::core::result::Result::Err({}),",
                fault.path()
            )
            .unwrap();
        }
        out.push_str("        }\n    }\n");
    }
    out.push_str("}\n");
}

fn storage<'a>(field: &Field, binding: &'a Binding) -> &'a str {
    match field {
        Field::Text {} => "::std::string::String",
        Field::U8 {} => "::core::primitive::u8",
        Field::Named { id } => &binding.types[id].name,
    }
}

fn input<'a>(field: &Field, binding: &'a Binding) -> &'a str {
    if matches!(field, Field::Text {}) {
        "&::core::primitive::str"
    } else {
        storage(field, binding)
    }
}

fn guard(rule: &Guard, natives: &BTreeMap<String, Member>) -> String {
    let field = &natives[rule.field()].name;
    let intervals = |items: &[Range]| {
        items
            .iter()
            .map(|r| format!("{}..={}", r.minimum, r.maximum))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    match rule {
        Guard::OneOf { values, .. } => {
            let choices = values
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(" | ");
            format!("::core::matches!({field}, {choices})")
        }
        Guard::Utf8ByteLength {
            minimum, maximum, ..
        } => {
            format!("({minimum}usize..={maximum}usize).contains(&{field}.len())")
        }
        Guard::AsciiBytes { ranges, .. } => {
            let choices = intervals(ranges);
            format!("{field}.bytes().all(|__byte| ::core::matches!(__byte, {choices}))")
        }
        Guard::ExcludeUnicode {
            controls, ranges, ..
        } => {
            let mut checks = Vec::new();
            if *controls {
                checks.push("::core::matches!(__scalar as u32, 0..=31 | 127..=159)".to_owned());
            }
            if !ranges.is_empty() {
                checks.push(format!(
                    "::core::matches!(__scalar as u32, {})",
                    intervals(ranges)
                ));
            }
            format!("!{field}.chars().any(|__scalar| {})", checks.join(" || "))
        }
    }
}

fn record(
    out: &mut String,
    fields: &BTreeMap<String, Field>,
    constructor: Option<&str>,
    guards: &[Guard],
    binding: &Binding,
    target: &Native,
) {
    let name = &target.name;
    let tuple = target.shape == Some(Shape::Tuple);
    if tuple {
        let id = &target.order[0];
        let public = if target.members[id].public {
            "pub "
        } else {
            ""
        };
        writeln!(
            out,
            "pub struct {name}({public}{});",
            storage(&fields[id], binding)
        )
        .unwrap();
    } else {
        writeln!(out, "pub struct {name} {{").unwrap();
        for id in &target.order {
            let field = &target.members[id];
            let public = if field.public { "pub " } else { "" };
            writeln!(
                out,
                "    {public}{}: {},",
                field.name,
                storage(&fields[id], binding)
            )
            .unwrap();
        }
        out.push_str("}\n");
    }
    if constructor.is_none()
        && target
            .members
            .values()
            .all(|member| member.getter.is_none())
    {
        return;
    }
    writeln!(out, "impl {name} {{").unwrap();
    if let Some(error) = constructor {
        let fault = &binding.errors[error];
        let method = target
            .constructor
            .as_ref()
            .expect("validated constructor binding");
        let arguments = target
            .order
            .iter()
            .map(|id| {
                format!(
                    "{}: {}",
                    target.members[id].name,
                    input(&fields[id], binding)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            out,
            "    pub fn {method}({arguments}) -> ::core::result::Result<Self, {}> {{",
            fault.error_type
        )
        .unwrap();
        for rule in guards {
            writeln!(
                out,
                "        if !({}) {{ return ::core::result::Result::Err({}); }}",
                guard(rule, &target.members),
                fault.path()
            )
            .unwrap();
        }
        let values = target
            .order
            .iter()
            .map(|id| {
                let field = &target.members[id].name;
                let value = if matches!(fields[id], Field::Text {}) {
                    format!("::std::string::String::from({field})")
                } else {
                    field.clone()
                };
                if tuple {
                    value
                } else {
                    format!("{field}: {value}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let product = if tuple {
            format!("Self({values})")
        } else {
            format!("Self {{ {values} }}")
        };
        writeln!(out, "        ::core::result::Result::Ok({product})\n    }}").unwrap();
    }
    for id in &target.order {
        let field = &target.members[id];
        if let Some(method) = &field.getter {
            let accessor = if tuple { "0" } else { field.name.as_str() };
            let native = storage(&fields[id], binding);
            let (ty, body) = match &fields[id] {
                Field::Text {} => (
                    "&::core::primitive::str".into(),
                    format!("self.{accessor}.as_str()"),
                ),
                Field::U8 {} => (native.to_owned(), format!("self.{accessor}")),
                Field::Named { id } if binding.types[id].traits.contains(&Derived::Copy) => {
                    (native.to_owned(), format!("self.{accessor}"))
                }
                Field::Named { .. } => (format!("&{native}"), format!("&self.{accessor}")),
            };
            writeln!(out, "    pub fn {method}(&self) -> {ty} {{ {body} }}").unwrap();
        }
    }
    out.push_str("}\n");
}

fn fixed_bytes(out: &mut String, length: u32, fault: &Fault, target: &Native) {
    let name = &target.name;
    let create = target
        .constructor
        .as_deref()
        .expect("validated constructor");
    let parse = target
        .parse
        .as_deref()
        .expect("validated sequence constructor");
    let view = target.view.as_deref().expect("validated borrow");
    let array = format!("[::core::primitive::u8; {length}]");
    let error = &fault.error_type;
    let rejected = fault.path();
    writeln!(out, "pub struct {name}(::zeroize::Zeroizing<{array}>);").unwrap();
    writeln!(out, "impl {name} {{").unwrap();
    writeln!(
        out,
        "    pub fn {create}(input: {array}) -> Self {{ Self(::zeroize::Zeroizing::new(input)) }}"
    )
    .unwrap();
    writeln!(out, "    pub fn {parse}(input: &[::core::primitive::u8]) -> ::core::result::Result<Self, {error}> {{").unwrap();
    writeln!(out, "        <{array} as ::core::convert::TryFrom<&[::core::primitive::u8]>>::try_from(input).map(Self::{create}).map_err(|_| {rejected})").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    pub fn {view}(&self) -> &{array} {{ &self.0 }}").unwrap();
    writeln!(out, "}}").unwrap();
}

pub(crate) fn render_plain_choice<'a>(
    out: &mut String,
    name: &str,
    cases: impl Iterator<Item = &'a str>,
    equality: Equality,
    debug: bool,
) {
    render_fieldless_choice(out, name, cases, equality, true, debug);
}

fn render_fieldless_choice<'a>(
    out: &mut String,
    name: &str,
    cases: impl Iterator<Item = &'a str>,
    equality: Equality,
    copy: bool,
    debug: bool,
) {
    let copy = if copy { ", Copy" } else { "" };
    let equality = match equality {
        Equality::None => "",
        Equality::Partial => ", PartialEq",
        Equality::Total => ", PartialEq, Eq",
    };
    let debug = if debug { ", Debug" } else { "" };
    writeln!(
        out,
        "#[derive(Clone{copy}{equality}{debug})]\npub enum {name} {{"
    )
    .unwrap();
    for case in cases {
        writeln!(out, "    {case},").unwrap();
    }
    out.push_str("}\n");
}

pub(super) fn render(graph: &Graph, binding: &Binding) -> String {
    let mut out = String::new();
    for (id, definition) in &graph.types {
        let target = &binding.types[id];
        if !matches!(
            definition,
            Definition::FixedBytes { .. } | Definition::Choice { .. }
        ) {
            derives(&mut out, target);
        }
        match definition {
            Definition::FixedBytes {
                length, failure, ..
            } => fixed_bytes(&mut out, *length, &binding.errors[failure], target),
            Definition::Choice {
                cases,
                equality,
                copy,
                debug,
            } => render_fieldless_choice(
                &mut out,
                &target.name,
                cases.iter().map(|case| binding.variants[case].as_str()),
                *equality,
                *copy,
                *debug,
            ),
            Definition::Enum { values, failure } => {
                enumeration(&mut out, values, failure.as_deref(), binding, target)
            }
            Definition::Record {
                fields,
                constructor,
                guards,
            } => record(
                &mut out,
                fields,
                constructor.as_deref(),
                guards,
                binding,
                target,
            ),
        }
    }
    out
}
