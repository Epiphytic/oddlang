//! Generate ordered, borrowed wire bounds before native values are copied.
use super::rust::ERROR;
use crate::wire_schema::{Binding, Definition, Field, Payload, Schema, Shape};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

pub(super) struct Bounds<'a> {
    functions: BTreeMap<&'a str, String>,
}

impl<'a> Bounds<'a> {
    pub fn new(schema: &'a Schema) -> Self {
        let mut checked = BTreeSet::new();
        loop {
            let added: Vec<_> = schema
                .types
                .iter()
                .filter_map(|definition| {
                    let id = definition.id();
                    (!checked.contains(id) && has_bounds(definition, &checked)).then_some(id)
                })
                .collect();
            if added.is_empty() {
                break;
            }
            checked.extend(added);
        }
        Self {
            functions: checked
                .into_iter()
                .enumerate()
                .map(|(index, id)| (id, format!("__oddlang_bounds_{index}")))
                .collect(),
        }
    }

    pub fn fields(&self, fields: &[Field], access: impl Fn(&Field) -> String) -> String {
        fields
            .iter()
            .map(|field| self.check(&field.shape, &access(field), "    "))
            .collect()
    }

    pub fn emit(&self, schema: &Schema, binding: &Binding) -> String {
        if self.functions.is_empty() {
            return String::new();
        }
        let mut text = String::new();
        for definition in &schema.types {
            let id = definition.id();
            let Some(helper) = self.functions.get(id) else {
                continue;
            };
            let shape = &binding.names[id];
            writeln!(
                text,
                "\n#[allow(dead_code)]\nfn {helper}(value: &{shape}, budget: &mut __OddlangWireBudget) -> ::core::result::Result<(), {ERROR}> {{"
            )
            .unwrap();
            match definition {
                Definition::Record { fields, .. } => {
                    for field in fields {
                        text.push_str(&self.check(
                            &field.shape,
                            &format!("&value.{}", binding.names[&field.id]),
                            "    ",
                        ));
                    }
                }
                Definition::Adjacent { variants, .. } => {
                    text.push_str("    match value {\n");
                    let mut count = 0;
                    for variant in variants {
                        let name = &binding.names[&variant.id];
                        match &variant.payload {
                            Payload::Newtype { shape: value } => {
                                let checks = self.check(value, "__oddlang_payload", "            ");
                                if checks.is_empty() {
                                    continue;
                                }
                                count += 1;
                                writeln!(text, "        {shape}::{name}(__oddlang_payload) => {{")
                                    .unwrap();
                                text.push_str(&checks);
                            }
                            Payload::Record { fields } => {
                                let mut checks = String::new();
                                let mut pattern = Vec::new();
                                for (index, field) in fields.iter().enumerate() {
                                    let value = format!("__oddlang_member_{index}");
                                    let code = self.check(&field.shape, &value, "            ");
                                    if !code.is_empty() {
                                        checks.push_str(&code);
                                        pattern
                                            .push(format!("{}: {value}", binding.names[&field.id]));
                                    }
                                }
                                if checks.is_empty() {
                                    continue;
                                }
                                count += 1;
                                writeln!(
                                    text,
                                    "        {shape}::{name} {{ {}, .. }} => {{",
                                    pattern.join(", ")
                                )
                                .unwrap();
                                text.push_str(&checks);
                            }
                            Payload::Unit {} => continue,
                        }
                        text.push_str("        }\n");
                    }
                    if count != variants.len() {
                        text.push_str("        _ => {}\n");
                    }
                    text.push_str("    }\n");
                }
                Definition::External { .. } => unreachable!("checks use defined shapes only"),
            }
            text.push_str("    ::core::result::Result::Ok(())\n}\n");
        }
        text
    }

    fn check(&self, shape: &Shape, value: &str, indent: &str) -> String {
        match shape {
            Shape::Text {} => format!("{indent}budget.take(({value}).len())?;\n"),
            Shape::List { item } | Shape::Optional { item } => {
                let child = format!("{indent}    ");
                let item_code = self.check(item, "__oddlang_item", &child);
                if matches!(shape, Shape::List { .. }) {
                    let mut result = format!("{indent}budget.take(({value}).len())?;\n");
                    if !item_code.is_empty() {
                        writeln!(result, "{indent}for __oddlang_item in ({value}).iter() {{")
                            .unwrap();
                        result.push_str(&item_code);
                        writeln!(result, "{indent}}}").unwrap();
                    }
                    result
                } else if item_code.is_empty() {
                    item_code
                } else {
                    format!(
                        "{indent}if let ::core::option::Option::Some(__oddlang_item) = ({value}).as_ref() {{\n{item_code}{indent}}}\n"
                    )
                }
            }
            Shape::BoundedBase64 { policy } => {
                format!("{indent}budget.binary({value}, {})?;\n", policy.max_bytes,)
            }
            Shape::BoundedBase64List { policy } => format!(
                "{indent}budget.binary_list({value}, {}, {}, {}, {})?;\n",
                policy.max_items,
                policy.max_item_bytes,
                policy.max_total_bytes,
                policy.per_item_overhead,
            ),
            Shape::Ref { id } => self
                .functions
                .get(id.as_str())
                .map_or_else(String::new, |function| {
                    format!("{indent}{function}({value}, budget)?;\n")
                }),
            _ => String::new(),
        }
    }
}

fn has_bounds(definition: &Definition, checked: &BTreeSet<&str>) -> bool {
    fn needed(shape: &Shape, checked: &BTreeSet<&str>) -> bool {
        match shape {
            Shape::Text {}
            | Shape::BoundedBase64 { .. }
            | Shape::BoundedBase64List { .. }
            | Shape::List { .. } => true,
            Shape::Optional { item } => needed(item, checked),
            Shape::Ref { id } => checked.contains(id.as_str()),
            _ => false,
        }
    }
    let fields_need = |fields: &[Field]| fields.iter().any(|field| needed(&field.shape, checked));
    match definition {
        Definition::Record { fields, .. } => fields_need(fields),
        Definition::Adjacent { variants, .. } => {
            variants.iter().any(|variant| match &variant.payload {
                Payload::Record { fields } => fields_need(fields),
                Payload::Newtype { shape } => needed(shape, checked),
                Payload::Unit {} => false,
            })
        }
        Definition::External { .. } => false,
    }
}

pub(super) fn runtime(limit: u32, size: &str, admission: &str) -> String {
    format!(
        r#"
const __ODDLANG_MAX_WIRE_BYTES: usize = {limit}usize;

struct __OddlangWireBytes {{
    bytes: ::std::vec::Vec<u8>,
    overflow: bool,
}}

impl ::std::io::Write for __OddlangWireBytes {{
    fn write(&mut self, bytes: &[u8]) -> ::std::io::Result<usize> {{
        if bytes.len() > __ODDLANG_MAX_WIRE_BYTES - self.bytes.len() {{
            self.overflow = true;
            return ::core::result::Result::Err(::std::io::Error::other("canonical wire budget"));
        }}
        self.bytes.extend_from_slice(bytes);
        ::core::result::Result::Ok(bytes.len())
    }}
    fn flush(&mut self) -> ::std::io::Result<()> {{
        ::core::result::Result::Ok(())
    }}
}}

fn __oddlang_serialize_frame<T: ::serde::Serialize>(
    value: &T,
) -> ::core::result::Result<::std::vec::Vec<u8>, {ERROR}> {{
    let mut writer = __OddlangWireBytes {{ bytes: ::std::vec::Vec::new(), overflow: false }};
    let status = ::serde_json::to_writer(&mut writer, value);
    if writer.overflow {{
        return ::core::result::Result::Err({size});
    }}
    status.map_err(|_| {admission})?;
    ::core::result::Result::Ok(writer.bytes)
}}

#[allow(dead_code)]
struct __OddlangWireBudget(usize);

#[allow(dead_code)]
impl __OddlangWireBudget {{
    fn take(&mut self, bytes: usize) -> ::core::result::Result<(), {ERROR}> {{
        self.0 = self.0.checked_sub(bytes).ok_or_else(|| {size})?;
        ::core::result::Result::Ok(())
    }}

    fn binary(&mut self, bytes: &[u8], maximum: u32) -> ::core::result::Result<(), {ERROR}> {{
        if bytes.len() as u64 > u64::from(maximum) {{
            return ::core::result::Result::Err({admission});
        }}
        self.take(bytes.len())
    }}

    fn binary_list(&mut self, items: &[::std::vec::Vec<u8>], max_items: u32, max_item: u32,
                   max_total: u32, overhead: u32) -> ::core::result::Result<(), {ERROR}> {{
        if items.len() as u64 > u64::from(max_items) {{
            return ::core::result::Result::Err({admission});
        }}
        self.take(items.len())?;
        let mut total = u64::from(max_total);
        for item in items {{
            let bytes = item.len() as u64;
            if bytes > u64::from(max_item) {{
                return ::core::result::Result::Err({admission});
            }}
            total = total.checked_sub(bytes + u64::from(overhead)).ok_or_else(|| {admission})?;
            self.take(item.len())?;
        }}
        ::core::result::Result::Ok(())
    }}
}}
"#
    )
}

pub(super) fn check_fields(
    output: &mut String,
    bounds: &Bounds<'_>,
    fields: &[Field],
    access: impl Fn(&Field) -> String,
) {
    let code = bounds.fields(fields, access);
    if !code.is_empty() {
        output.push_str("    let budget = &mut __OddlangWireBudget(__ODDLANG_MAX_WIRE_BYTES);\n");
        output.push_str(&code);
    }
}
