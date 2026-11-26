use super::*;
use std::fmt::Write as _;

pub(super) fn render(g: &Graph, b: &Binding) -> Result<String> {
    let values = g.constants()?;
    let mut out = String::new();
    for c in &g.constants {
        let value = values[c.id.as_str()];
        let (ty, max) = match b.constant_types[&c.id] {
            Width::U8 => ("u8", u8::MAX as u64),
            Width::U16 => ("u16", u16::MAX as u64),
            Width::U32 => ("u32", u32::MAX as u64),
            Width::U64 => ("u64", u64::MAX),
            Width::Usize => ("usize", u64::MAX),
        };
        if value > max {
            return Err(format!("constant exceeds native type: {}", c.id));
        }
        writeln!(
            out,
            "pub const {}: ::core::primitive::{ty} = {value};",
            b.name(&c.id)?
        )
        .unwrap();
    }
    for e in &g.errors {
        if b.external_error_types.contains_key(&e.id) {
            continue;
        }
        let name = b.name(&e.id)?;
        writeln!(
            out,
            "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum {name} {{"
        )
        .unwrap();
        let variants: BTreeMap<_, _> = e.variants.iter().map(|v| (&v.id, v)).collect();
        for id in &b.variant_order[&e.id] {
            let payload = match variants[id].payload {
                Payload::None => "",
                Payload::Text => "(&'static ::core::primitive::str)",
                Payload::Bool => "(::core::primitive::bool)",
                Payload::U64 => "(::core::primitive::u64)",
            };
            writeln!(out, "    {}{payload},", b.name(id)?).unwrap();
        }
        writeln!(out,"}}\nimpl ::core::fmt::Display for {name} {{\n    fn fmt(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{\n        match self {{").unwrap();
        for id in &b.variant_order[&e.id] {
            let v = variants[id];
            let arg = if v.payload == Payload::None {
                ""
            } else {
                "(_payload)"
            };
            writeln!(out, "            Self::{}{arg} => {{", b.name(id)?).unwrap();
            for segment in &v.message {
                match segment {
                    Segment::Text { text } => {
                        writeln!(out, "                __formatter.write_str({text:?})?;").unwrap()
                    }
                    Segment::Payload { .. } => writeln!(
                        out,
                        "                ::core::write!(__formatter, \"{{}}\", _payload)?;"
                    )
                    .unwrap(),
                }
            }
            out.push_str("                ::core::result::Result::Ok(())\n            }\n");
        }
        writeln!(
            out,
            "        }}\n    }}\n}}\nimpl ::core::error::Error for {name} {{}}"
        )
        .unwrap();
    }
    for a in &g.results {
        writeln!(
            out,
            "pub type {}<__Value> = ::core::result::Result<__Value, {}>;",
            b.name(&a.id)?,
            b.external_error_types
                .get(&a.error)
                .map(String::as_str)
                .unwrap_or(b.name(&a.error)?)
        )
        .unwrap();
    }
    for e in &g.string_enums {
        if e.numbers.is_some() {
            out.push_str("#[repr(u8)]\n");
        }
        writeln!(out,"#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]\npub enum {} {{",b.name(&e.id)?).unwrap();
        let variants: BTreeMap<_, _> = e.variants.iter().map(|v| (&v.id, v)).collect();
        for id in &b.variant_order[&e.id] {
            let tag = e
                .numbers
                .as_ref()
                .map(|n| format!(" = {}", n.tags[id]))
                .unwrap_or_default();
            writeln!(
                out,
                "    #[serde(rename = {:?})]\n    {}{tag},",
                variants[id].wire,
                b.name(id)?
            )
            .unwrap();
        }
        out.push_str("}\n");
        if let Some(n) = &e.numbers {
            let name = b.name(&e.id)?;
            let (owner, variant) = g.error_variant(&n.unknown.id)?;
            let ty = b
                .external_error_types
                .get(&owner.id)
                .map(String::as_str)
                .unwrap_or(b.name(&owner.id)?);
            let variant = b.name(&variant.id)?;
            let arg = match n.unknown.argument.as_ref() {
                Some(serde_json::Value::String(s)) => format!("({s:?})"),
                Some(v) => format!("({v})"),
                None => String::new(),
            };
            writeln!(out,"impl ::core::convert::TryFrom<::core::primitive::u8> for {name} {{\n    type Error = {ty};\n    fn try_from(__byte: ::core::primitive::u8) -> ::core::result::Result<Self,{ty}> {{\n        match __byte {{").unwrap();
            for (id, tag) in &n.tags {
                writeln!(
                    out,
                    "            {tag} => ::core::result::Result::Ok(Self::{}),",
                    b.name(id)?
                )
                .unwrap();
            }
            if n.tags.len() < 256 {
                writeln!(
                    out,
                    "            _ => ::core::result::Result::Err({ty}::{variant}{arg}),"
                )
                .unwrap();
            }
            out.push_str("        }\n    }\n}\n");
        }
    }
    Ok(out)
}
