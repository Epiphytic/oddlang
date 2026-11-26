use super::{Presence, Response, ResponseField, unique_keys};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum ValueType {
    Primitive(Primitive),
    Shape(Shape),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Primitive {
    Bool,
    String,
    Json,
    Null,
    U8,
    U32,
    U64,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Shape {
    Ref {
        id: String,
    },
    List {
        item: Box<ValueType>,
    },
    FixedBytes {
        length: u8,
    },
    LowerHex {
        bytes: u8,
    },
    AsciiAtom {
        maximum_bytes: u16,
        punctuation: String,
    },
    LiteralU8 {
        value: u8,
    },
    PaddedBase64 {
        maximum_bytes: u32,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Declaration {
    Record {
        id: String,
        fields: Vec<ResponseField>,
    },
    Tagged {
        id: String,
        tag: String,
        content: Option<String>,
        variants: Vec<Variant>,
    },
    Strings {
        id: String,
        variants: Vec<Variant>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Variant {
    id: String,
    tag: String,
    fields: Option<Vec<ResponseField>>,
}

impl Declaration {
    fn id(&self) -> &str {
        match self {
            Self::Record { id, .. } | Self::Tagged { id, .. } | Self::Strings { id, .. } => id,
        }
    }
}

fn insert_id(id: &str, identities: &mut BTreeSet<String>) -> Result<(), String> {
    oxigraph::model::NamedNode::new(id)
        .map_err(|_| format!("invalid reply declaration identity {id}"))?;
    if !identities.insert(id.to_owned()) {
        return Err(format!("duplicate reply declaration identity {id}"));
    }
    Ok(())
}

fn validate_fields(
    fields: &[ResponseField],
    typed: bool,
    identities: &mut BTreeSet<String>,
) -> Result<(), String> {
    if !unique_keys(fields.iter().map(|field| field.key.as_str())) {
        return Err("duplicate or empty reply field name".into());
    }
    for field in fields {
        if typed || field.id.is_some() {
            insert_id(
                field
                    .id
                    .as_deref()
                    .ok_or("typed reply field requires an identity")?,
                identities,
            )?;
        }
        if field.default && matches!(field.presence, Presence::Required) {
            return Err("only nullable reply fields can default to null".into());
        }
    }
    Ok(())
}

fn visit_type(
    value: &ValueType,
    declarations: &BTreeMap<&str, &Declaration>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), String> {
    match value {
        ValueType::Primitive(_) => Ok(()),
        ValueType::Shape(Shape::Ref { id }) => {
            if visited.contains(id) {
                return Ok(());
            }
            if !visiting.insert(id.clone()) {
                return Err(format!("cyclic reply type {id}"));
            }
            let declaration = declarations
                .get(id.as_str())
                .ok_or_else(|| format!("unknown reply type {id}"))?;
            let visit_fields = |fields: &[ResponseField],
                                visiting: &mut BTreeSet<String>,
                                visited: &mut BTreeSet<String>| {
                for field in fields {
                    visit_type(&field.kind, declarations, visiting, visited)?;
                }
                Ok::<_, String>(())
            };
            match declaration {
                Declaration::Record { fields, .. } => visit_fields(fields, visiting, visited)?,
                Declaration::Tagged { variants, .. } => {
                    for variant in variants {
                        if let Some(fields) = &variant.fields {
                            visit_fields(fields, visiting, visited)?;
                        }
                    }
                }
                Declaration::Strings { .. } => {}
            }
            visiting.remove(id);
            visited.insert(id.clone());
            Ok(())
        }
        ValueType::Shape(Shape::List { item }) => visit_type(item, declarations, visiting, visited),
        ValueType::Shape(Shape::FixedBytes { length }) if (1..=32).contains(length) => Ok(()),
        ValueType::Shape(Shape::LowerHex { bytes }) if (1..=64).contains(bytes) => Ok(()),
        ValueType::Shape(Shape::AsciiAtom {
            maximum_bytes,
            punctuation,
        }) if *maximum_bytes > 0 && punctuation.bytes().all(|byte| byte.is_ascii_punctuation()) => {
            Ok(())
        }
        ValueType::Shape(Shape::LiteralU8 { .. }) => Ok(()),
        ValueType::Shape(Shape::PaddedBase64 { maximum_bytes })
            if (1..=16_777_216).contains(maximum_bytes) =>
        {
            Ok(())
        }
        _ => Err("invalid bounded reply value type".into()),
    }
}

pub(super) fn validate(response: &Response) -> Result<(), String> {
    let mut identities = BTreeSet::new();
    let typed = !response.declarations.is_empty()
        || response.ordered_sequence_minimum.is_some()
        || response.id.is_some()
        || response.fields.iter().any(|field| {
            field.id.is_some() || field.default || matches!(field.kind, ValueType::Shape(_))
        });
    if typed || response.id.is_some() {
        insert_id(
            response
                .id
                .as_deref()
                .ok_or("typed reply needs a root declaration identity")?,
            &mut identities,
        )?;
    }
    validate_fields(&response.fields, typed, &mut identities)?;
    if let Some(minimum) = response.ordered_sequence_minimum {
        if minimum > response.fields.len()
            || response.fields.iter().enumerate().any(|(index, field)| {
                if index < minimum {
                    field.default
                } else {
                    matches!(field.presence, Presence::Required)
                }
            })
        {
            return Err("invalid nullable suffix for ordered reply sequence".into());
        }
    }
    let mut declarations = BTreeMap::new();
    for declaration in &response.declarations {
        let id = declaration.id();
        insert_id(id, &mut identities)?;
        declarations.insert(id, declaration);
        match declaration {
            Declaration::Record { fields, .. } => validate_fields(fields, true, &mut identities)?,
            Declaration::Tagged {
                tag,
                content,
                variants,
                ..
            } => {
                if tag.is_empty()
                    || content
                        .as_ref()
                        .is_some_and(|value| value.is_empty() || value == tag)
                {
                    return Err("invalid reply discriminator keys".into());
                }
                validate_variants(variants, Some(tag), content.is_some(), &mut identities)?;
            }
            Declaration::Strings { variants, .. } => {
                validate_variants(variants, None, false, &mut identities)?
            }
        }
    }
    let mut visited = BTreeSet::new();
    for field in &response.fields {
        visit_type(
            &field.kind,
            &declarations,
            &mut BTreeSet::new(),
            &mut visited,
        )?;
    }
    if visited.len() != declarations.len() {
        return Err("unreferenced reply declaration".into());
    }
    Ok(())
}

fn validate_variants(
    variants: &[Variant],
    tag: Option<&String>,
    adjacent: bool,
    identities: &mut BTreeSet<String>,
) -> Result<(), String> {
    if variants.is_empty() || !unique_keys(variants.iter().map(|variant| variant.tag.as_str())) {
        return Err("duplicate or empty reply variant tag".into());
    }
    for variant in variants {
        insert_id(&variant.id, identities)?;
        if let Some(fields) = &variant.fields {
            if tag.is_none() || (!adjacent && fields.iter().any(|field| Some(&field.key) == tag)) {
                return Err("invalid tagged reply variant fields".into());
            }
            validate_fields(fields, true, identities)?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Renderer {
    names: BTreeMap<String, String>,
    helpers: String,
    index: usize,
}

impl Renderer {
    fn ty(&mut self, value: &ValueType) -> String {
        match value {
            ValueType::Primitive(primitive) => match primitive {
                Primitive::Bool => "bool",
                Primitive::String => "String",
                Primitive::Json => "serde_json::Value",
                Primitive::Null => "()",
                Primitive::U8 => "u8",
                Primitive::U32 => "u32",
                Primitive::U64 => "u64",
            }
            .to_owned(),
            ValueType::Shape(Shape::Ref { id }) => self.names[id].clone(),
            ValueType::Shape(Shape::List { item }) => format!("Vec<{}>", self.ty(item)),
            ValueType::Shape(Shape::FixedBytes { length }) => format!("[u8; {length}]"),
            ValueType::Shape(shape) => self.checked_value(shape),
        }
    }

    fn checked_value(&mut self, shape: &Shape) -> String {
        let index = self.index;
        self.index += 1;
        let name = format!("ReplyValue{index}");
        let (raw, native, body) = match shape {
            Shape::LowerHex { bytes } => (
                "String",
                "String",
                format!(
                    "if value.len() != {} || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {{ return Err(\"lowercase hexadecimal\"); }} Ok(Self(value))",
                    u16::from(*bytes) * 2
                ),
            ),
            Shape::AsciiAtom {
                maximum_bytes,
                punctuation,
            } => (
                "String",
                "String",
                format!(
                    "if value.is_empty() || value.len() > {maximum_bytes} || !value.bytes().all(|b| b.is_ascii_alphanumeric() || {punctuation:?}.as_bytes().contains(&b)) {{ return Err(\"ASCII atom\"); }} Ok(Self(value))"
                ),
            ),
            Shape::LiteralU8 { value } => (
                "u8",
                "u8",
                format!(
                    "if value != {value} {{ return Err(\"unsigned literal\"); }} Ok(Self(value))"
                ),
            ),
            Shape::PaddedBase64 { maximum_bytes } => (
                "String",
                "String",
                format!(
                    "use base64::{{Engine as _, engine::general_purpose::STANDARD}}; if value.len() > {} {{ return Err(\"base64 size\"); }} let decoded = STANDARD.decode(&value).map_err(|_| \"base64\")?; if decoded.len() > {maximum_bytes} || STANDARD.encode(decoded) != value {{ return Err(\"canonical base64\"); }} Ok(Self(value))",
                    maximum_bytes.div_ceil(3) * 4
                ),
            ),
            _ => unreachable!("only constrained primitives need a native adapter"),
        };
        writeln!(self.helpers,
            "#[derive(serde::Deserialize, serde::Serialize)]\n#[serde(try_from = {raw:?})]\nstruct {name}({native});\nimpl TryFrom<{raw}> for {name} {{\n    type Error = &'static str;\n    fn try_from(value: {raw}) -> Result<Self, Self::Error> {{ {body} }}\n}}"
        ).unwrap();
        name
    }

    fn fields(&mut self, out: &mut String, fields: &[ResponseField], minimum: Option<usize>) {
        for (index, field) in fields.iter().enumerate() {
            let ty = self.ty(&field.kind);
            writeln!(out, "    #[serde(rename = {:?})]", field.key).unwrap();
            if field.default || minimum.is_some_and(|limit| index >= limit) {
                out.push_str("    #[serde(default)]\n");
            }
            if matches!(field.presence, Presence::OmitNull) {
                out.push_str("    #[serde(skip_serializing_if = \"Option::is_none\")]\n");
            }
            let ty = if matches!(field.presence, Presence::Required) {
                ty
            } else {
                format!("Option<{ty}>")
            };
            writeln!(out, "    value_{index}: {ty},").unwrap();
        }
    }
}

pub(super) fn render(response: &Response, out: &mut String) {
    let names = response
        .declarations
        .iter()
        .enumerate()
        .map(|(index, declaration)| (declaration.id().to_owned(), format!("ReplyType{index}")))
        .collect();
    let mut renderer = Renderer {
        names,
        ..Renderer::default()
    };
    const DERIVE: &str = "\n#[derive(serde::Deserialize, serde::Serialize)]";
    for declaration in &response.declarations {
        let name = renderer.names[declaration.id()].clone();
        writeln!(out, "{DERIVE}").unwrap();
        match declaration {
            Declaration::Record { fields, .. } => {
                writeln!(out, "#[serde(deny_unknown_fields)]\nstruct {name} {{").unwrap();
                renderer.fields(out, fields, None);
            }
            Declaration::Tagged {
                tag,
                content,
                variants,
                ..
            } => {
                let content = content
                    .as_ref()
                    .map_or_else(String::new, |key| format!(", content = {key:?}"));
                writeln!(
                    out,
                    "#[serde(tag = {tag:?}{content}, deny_unknown_fields)]\nenum {name} {{"
                )
                .unwrap();
                render_variants(&mut renderer, out, variants);
            }
            Declaration::Strings { variants, .. } => {
                writeln!(out, "enum {name} {{").unwrap();
                render_variants(&mut renderer, out, variants);
            }
        }
        writeln!(out, "}}").unwrap();
    }
    writeln!(
        out,
        "{DERIVE}\n#[serde(deny_unknown_fields)]\nstruct Response {{"
    )
    .unwrap();
    renderer.fields(out, &response.fields, response.ordered_sequence_minimum);
    writeln!(out, "}}").unwrap();
    out.push_str(&renderer.helpers);
}

fn render_variants(renderer: &mut Renderer, out: &mut String, variants: &[Variant]) {
    for (index, variant) in variants.iter().enumerate() {
        writeln!(out, "    #[serde(rename = {:?})]", variant.tag).unwrap();
        if let Some(fields) = &variant.fields {
            writeln!(out, "    Variant{index} {{").unwrap();
            renderer.fields(out, fields, None);
            writeln!(out, "    }},").unwrap();
        } else {
            writeln!(out, "    Variant{index},").unwrap();
        }
    }
}

pub(super) fn obligations(response: &Response) -> Option<Value> {
    let id = response.id.as_ref()?;
    let mut rows = vec![json!({
        "declaration":id, "role":"reply-envelope", "fields":response.fields,
        "orderedSequenceMinimum":response.ordered_sequence_minimum.unwrap_or(response.fields.len()),
        "orderedSequenceMaximum":response.fields.len(), "unknownFields":"reject",
        "successField":response.success_field, "successExit":response.success_exit, "failureExit":response.failure_exit,
    })];
    let add_fields = |fields: &[ResponseField], rows: &mut Vec<Value>| {
        for (position, field) in fields.iter().enumerate() {
            rows.push(json!({"declaration":field.id,"role":"reply-field","position":position,"value":field}));
        }
    };
    add_fields(&response.fields, &mut rows);
    for declaration in &response.declarations {
        rows.push(json!({"declaration":declaration.id(),"role":"reply-type","type":declaration}));
        match declaration {
            Declaration::Record { fields, .. } => add_fields(fields, &mut rows),
            Declaration::Tagged { variants, .. } | Declaration::Strings { variants, .. } => {
                for variant in variants {
                    rows.push(
                        json!({"declaration":variant.id,"role":"reply-case","variant":variant}),
                    );
                    if let Some(fields) = &variant.fields {
                        add_fields(fields, &mut rows);
                    }
                }
            }
        }
    }
    Some(
        json!({"format":"oddlang-local-reply-obligations-v1","qualification":"independent-witness-required","obligations":rows}),
    )
}
