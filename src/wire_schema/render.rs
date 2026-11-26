use super::{
    BinaryList, Binding, Definition, Field, Guard, Payload, Predicate, Schema, Shape, Variant,
};
use std::{collections::BTreeMap, fmt::Write as _};

enum Adapter<'a> {
    Hex { bytes: usize, diagnostic: &'a str },
    Binary(&'a BinaryList),
}

pub(super) fn project(schema: &Schema, binding: &Binding) -> String {
    let mut text = String::new();
    let mut adapters = Vec::new();
    for definition in &schema.types {
        let id = definition.id();
        if matches!(definition, Definition::External { .. }) {
            continue;
        }
        let name = &binding.names[id];
        let mut derives: Vec<_> = binding
            .derives
            .get(id)
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        derives.extend(["::serde::Serialize", "::serde::Deserialize"]);
        writeln!(text, "#[derive({})]", derives.join(", ")).unwrap();
        match definition {
            Definition::Record { fields, .. } => {
                writeln!(text, "#[serde(deny_unknown_fields)]\npub struct {name} {{").unwrap();
                write_fields(&mut text, fields, binding, &mut adapters, "    ", true);
                text.push_str("}\n\n");
                let guards: Vec<_> = schema.guards.iter().filter(|g| g.owner == id).collect();
                if !guards.is_empty() {
                    guard_start(&mut text, name);
                    let selectors = fields
                        .iter()
                        .map(|field| {
                            (
                                field.id.as_str(),
                                (format!("self.{}", binding.names[&field.id]), false),
                            )
                        })
                        .collect();
                    write_guards(&mut text, &guards, &selectors, "        ");
                    guard_end(&mut text);
                }
            }
            Definition::Adjacent {
                tag,
                content,
                variants,
                ..
            } => {
                writeln!(text,
                    "#[serde(tag = {tag:?}, content = {content:?}, deny_unknown_fields)]\npub enum {name} {{"
                ).unwrap();
                for variant in variants {
                    let rust = &binding.names[&variant.id];
                    writeln!(text, "    #[serde(rename = {:?})]", variant.name).unwrap();
                    match &variant.payload {
                        Payload::Unit {} => writeln!(text, "    {rust},").unwrap(),
                        Payload::Newtype { shape } => {
                            writeln!(text, "    {rust}({}),", ty(shape, binding)).unwrap()
                        }
                        Payload::Record { fields } => {
                            writeln!(text, "    {rust} {{").unwrap();
                            write_fields(
                                &mut text,
                                fields,
                                binding,
                                &mut adapters,
                                "        ",
                                false,
                            );
                            text.push_str("    },\n");
                        }
                    }
                }
                text.push_str("}\n\n");
                union_guards(&mut text, schema, id, name, variants, binding);
            }
            Definition::External { .. } => unreachable!("external definitions were skipped"),
        }
    }
    if adapters
        .iter()
        .any(|item| matches!(item, Adapter::Hex { .. }))
    {
        text.push_str(include_str!("hex.rs"));
    }
    if adapters
        .iter()
        .any(|item| matches!(item, Adapter::Binary(_)))
    {
        text.push_str(include_str!("base64_list.rs"));
    }
    for (index, adapter) in adapters.iter().enumerate() {
        match adapter {
            Adapter::Hex { bytes, diagnostic } => {
                writeln!(text, r#"
mod __oddlang_wire_adapter_{index} {{
    pub fn serialize<S: ::serde::Serializer>(value: &[u8; {bytes}], writer: S) -> Result<S::Ok, S::Error> {{
        super::__oddlang_wire_hex::serialize(value, writer)
    }}
    pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(reader: D) -> Result<[u8; {bytes}], D::Error> {{
        super::__oddlang_wire_hex::deserialize(reader, {diagnostic:?})
    }}
}}"#).unwrap();
            }
            Adapter::Binary(policy) => write_binary_adapter(&mut text, index, policy),
        }
    }
    text
}

fn write_binary_adapter(text: &mut String, index: usize, policy: &BinaryList) {
    writeln!(text, r#"
mod __oddlang_wire_adapter_{index} {{
    use super::__oddlang_wire_base64_list as codec;
    pub fn serialize<S: ::serde::Serializer>(value: &[Vec<u8>], writer: S) -> Result<S::Ok, S::Error> {{
        codec::serialize(value, writer)
    }}
    pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(reader: D) -> Result<Vec<Vec<u8>>, D::Error> {{
        codec::deserialize(reader, codec::Policy {{
            items: {items}, item_bytes: {item}, total_bytes: {total}, overhead: {overhead},
            list: {list:?}, encoded: {encoded:?}, decoded: {decoded:?}, syntax: {syntax:?},
        }})
    }}
}}"#,
        items=policy.max_items, item=policy.max_item_bytes,
        total=policy.max_total_bytes, overhead=policy.per_item_overhead,
        list=policy.list_limit_message, encoded=policy.encoded_limit_message,
        decoded=policy.decoded_limit_message, syntax=policy.syntax_message,
    ).unwrap();
}

fn write_fields<'a>(
    text: &mut String,
    fields: &'a [Field],
    binding: &Binding,
    adapters: &mut Vec<Adapter<'a>>,
    indent: &str,
    public: bool,
) {
    let visibility = if public { "pub " } else { "" };
    for field in fields {
        write!(text, "{indent}#[serde(rename = {:?}", field.name).unwrap();
        let adapter = match &field.shape {
            Shape::LowerHex { bytes, diagnostic } => Some(Adapter::Hex {
                bytes: *bytes,
                diagnostic,
            }),
            Shape::BoundedBase64List { policy } => Some(Adapter::Binary(policy)),
            _ => None,
        };
        if let Some(adapter) = adapter {
            write!(
                text,
                ", with = {:?}",
                format!("__oddlang_wire_adapter_{}", adapters.len())
            )
            .unwrap();
            adapters.push(adapter);
        }
        writeln!(
            text,
            ")]\n{indent}{visibility}{}: {},",
            binding.names[&field.id],
            ty(&field.shape, binding)
        )
        .unwrap();
    }
}

fn ty(shape: &Shape, binding: &Binding) -> String {
    match shape {
        Shape::Bool {} => "::core::primitive::bool".into(),
        Shape::Text {} => "::std::string::String".into(),
        Shape::Unsigned { bits } => format!("::core::primitive::u{bits}"),
        Shape::Optional { item } => format!("::core::option::Option<{}>", ty(item, binding)),
        Shape::List { item } => format!("::std::vec::Vec<{}>", ty(item, binding)),
        Shape::Ref { id } => binding
            .names
            .get(id)
            .unwrap_or_else(|| &binding.externals[id])
            .clone(),
        Shape::LowerHex { bytes, .. } => format!("[::core::primitive::u8; {bytes}]"),
        Shape::BoundedBase64List { .. } => {
            "::std::vec::Vec<::std::vec::Vec<::core::primitive::u8>>".into()
        }
        Shape::Unresolved { .. } => unreachable!("validated complete wire schema"),
    }
}

fn guard_start(text: &mut String, name: &str) {
    writeln!(text, "impl {name} {{\n    pub fn validate_wire(&self) -> ::core::result::Result<(), &'static str> {{").unwrap();
}

fn guard_end(text: &mut String) {
    text.push_str("        ::core::result::Result::Ok(())\n    }\n}\n\n");
}

fn union_guards(
    text: &mut String,
    schema: &Schema,
    id: &str,
    name: &str,
    variants: &[Variant],
    binding: &Binding,
) {
    if !schema.guards.iter().any(|guard| guard.owner == id) {
        return;
    }
    guard_start(text, name);
    for variant in variants {
        let guards: Vec<_> = schema
            .guards
            .iter()
            .filter(|guard| {
                guard.owner == id && guard.variant.as_deref() == Some(variant.id.as_str())
            })
            .collect();
        if guards.is_empty() {
            continue;
        }
        let Payload::Record { fields } = &variant.payload else {
            unreachable!("validated guard owner");
        };
        let singleton = variants.len() == 1;
        let pattern = if singleton { "let" } else { "if let" };
        writeln!(
            text,
            "        {pattern} Self::{} {{",
            binding.names[&variant.id]
        )
        .unwrap();
        let mut selectors = BTreeMap::new();
        for (index, field) in fields.iter().enumerate() {
            if guards.iter().any(|guard| {
                references(&guard.require, &field.id)
                    || guard
                        .when
                        .as_ref()
                        .is_some_and(|when| references(when, &field.id))
            }) {
                writeln!(
                    text,
                    "            {}: __field_{index},",
                    binding.names[&field.id]
                )
                .unwrap();
                selectors.insert(field.id.as_str(), (format!("__field_{index}"), true));
            }
        }
        if singleton {
            text.push_str("            ..\n        } = self;\n");
            write_guards(text, &guards, &selectors, "        ");
        } else {
            text.push_str("            ..\n        } = self {\n");
            write_guards(text, &guards, &selectors, "            ");
            text.push_str("        }\n");
        }
    }
    guard_end(text);
}

fn references(predicate: &Predicate, id: &str) -> bool {
    match predicate {
        Predicate::Bool { field, .. }
        | Predicate::Present { field }
        | Predicate::UnsignedRange { field, .. }
        | Predicate::Count { field, .. }
        | Predicate::Unique { field } => field == id,
        Predicate::AtMost { left, right } => left == id || right == id,
    }
}

fn expression(predicate: &Predicate, selectors: &BTreeMap<&str, (String, bool)>) -> String {
    let select = |id: &str| &selectors[id].0;
    let scalar = |id: &str| {
        let (expr, borrowed) = &selectors[id];
        if *borrowed {
            format!("*{expr}")
        } else {
            expr.clone()
        }
    };
    match predicate {
        Predicate::Bool { field, value } if *value => scalar(field),
        Predicate::Bool { field, .. } => format!("!{}", scalar(field)),
        Predicate::Present { field } => format!("{}.is_some()", select(field)),
        Predicate::UnsignedRange {
            field,
            minimum,
            maximum,
        } => format!(
            "({minimum}..={maximum}).contains(&::core::primitive::u64::from({}))",
            scalar(field)
        ),
        Predicate::Count {
            field,
            minimum,
            maximum,
        } => {
            format!("({minimum}..={maximum}).contains(&{}.len())", select(field))
        }
        Predicate::Unique { field } => format!(
            "{{ let mut seen = ::std::collections::BTreeSet::new(); {}.iter().all(|item| seen.insert(item)) }}",
            select(field)
        ),
        Predicate::AtMost { left, right } => format!(
            "::core::primitive::u64::from({}) <= ::core::primitive::u64::from({})",
            scalar(left),
            scalar(right)
        ),
    }
}

fn write_guards(
    text: &mut String,
    guards: &[&Guard],
    selectors: &BTreeMap<&str, (String, bool)>,
    indent: &str,
) {
    for guard in guards {
        let required = expression(&guard.require, selectors);
        write!(text, "{indent}if ").unwrap();
        if let Some(when) = &guard.when {
            write!(text, "{} && ", expression(when, selectors)).unwrap();
        }
        writeln!(
            text,
            "!({required}) {{\n{indent}    return ::core::result::Result::Err({:?});\n{indent}}}",
            guard.error
        )
        .unwrap();
    }
}
