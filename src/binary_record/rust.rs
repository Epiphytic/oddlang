use super::{
    Binding, Encoding, Graph,
    extension::{self, Payload},
};
use std::fmt::Write as _;

pub(super) fn render(graph: &Graph, binding: &Binding) -> String {
    let mut text = String::new();
    let name = &binding.name;
    let error = &binding.error_type;
    let max = graph.maximum;
    let fault = |id: &str| {
        let fault = &binding.errors[id];
        let variant = &fault.variant;
        match &fault.argument {
            Some(argument) => format!("{error}::{variant}({argument:?})"),
            None => format!("{error}::{variant}"),
        }
    };
    let slots = &graph.layout;
    extension::render_records(&mut text, graph, binding);
    writeln!(
        text,
        "#[derive(Clone, PartialEq, Eq)]\npub struct {name} {{"
    )
    .unwrap();
    for slot in slots.iter().filter(|slot| slot.encoding.field()) {
        let ty = match &slot.encoding {
            Encoding::UnsignedBe { bits, .. } => format!("::core::primitive::u{bits}"),
            Encoding::Bool { .. } => "::core::primitive::bool".into(),
            Encoding::SignedBe { bits } => extension::integer(*bits, true),
            Encoding::Inline { id } => binding.names[id].clone(),
            Encoding::Nested { record, .. } => binding.external_records[record].path.clone(),
            Encoding::Optional { payload, .. } => {
                format!("::core::option::Option<{}>", payload.ty(binding))
            }
            Encoding::FixedBytes { size } => format!("[::core::primitive::u8; {size}]"),
            Encoding::Bytes { .. } => "::std::vec::Vec<::core::primitive::u8>".into(),
            Encoding::AsciiAtom { .. } => "::std::string::String".into(),
            Encoding::EnumU8 { external, .. } => binding.externals[external].path.clone(),
            _ => unreachable!("literal has no native field"),
        };
        writeln!(text, "    pub {}: {ty},", binding.names[&slot.id]).unwrap();
    }
    writeln!(
        text,
        "}}\nimpl {name} {{\n    pub fn validate(&self) -> ::core::result::Result<(), {error}> {{"
    )
    .unwrap();
    let value_fault = fault(&graph.errors.value);
    for slot in slots.iter().filter(|slot| slot.encoding.field()) {
        let field = &binding.names[&slot.id];
        if matches!(slot.encoding, Encoding::Nested { .. }) {
            writeln!(text, "        self.{field}.validate()?;").unwrap();
            continue;
        }
        let mut error = value_fault.clone();
        let rule = match &slot.encoding {
            Encoding::UnsignedBe {
                minimum,
                maximum,
                value_error,
                ..
            } => {
                if let Some(id) = value_error {
                    error = fault(id);
                }
                format!("({minimum}..={maximum}).contains(&self.{field})")
            }
            Encoding::Bytes {
                minimum,
                maximum,
                excludes,
                value_error,
                ..
            } => {
                if let Some(id) = value_error {
                    error = fault(id);
                }
                let mut rule = format!("({minimum}..={maximum}).contains(&self.{field}.len())");
                for byte in excludes {
                    write!(rule, " && !self.{field}.contains(&{byte})").unwrap();
                }
                rule
            }
            Encoding::AsciiAtom {
                minimum,
                maximum,
                punctuation,
                ..
            } => atom(
                &format!("self.{field}.as_bytes()"),
                *minimum,
                *maximum,
                punctuation,
            ),
            _ => continue,
        };
        writeln!(
            text,
            "        if !({rule}) {{ return ::core::result::Result::Err({error}); }}"
        )
        .unwrap();
    }
    for guard in &graph.guards {
        writeln!(
            text,
            "        if !({}) {{ return ::core::result::Result::Err({}); }}",
            guard.expression(binding),
            fault(&guard.error)
        )
        .unwrap();
    }
    for constraint in &graph.constraints {
        writeln!(
            text,
            "        if !({}) {{ return ::core::result::Result::Err({}); }}",
            constraint.rust(graph, binding),
            fault(&constraint.error)
        )
        .unwrap();
    }
    writeln!(text, "        ::core::result::Result::Ok(())\n    }}").unwrap();

    writeln!(text, "    pub fn encode(&self) -> ::core::result::Result<::std::vec::Vec<::core::primitive::u8>, {error}> {{\n        self.validate()?;").unwrap();
    let mut fixed = if graph.integrity.is_some() { 32 } else { 0 };
    let mut sizes = String::new();
    for (index, slot) in slots.iter().enumerate() {
        match &slot.encoding {
            Encoding::Bytes { .. } | Encoding::AsciiAtom { .. } => {
                fixed += 4;
                write!(sizes, " + self.{}.len()", binding.names[&slot.id]).unwrap();
            }
            Encoding::Optional { payload, .. } => {
                fixed += 1;
                let name = &binding.names[&slot.id];
                let width = payload
                    .width(&graph.records)
                    .expect("validated optional width");
                write!(
                    sizes,
                    " + if self.{name}.is_some() {{ {width} }} else {{ 0 }}"
                )
                .unwrap();
            }
            Encoding::Nested {
                maximum, overflow, ..
            } => {
                fixed += 4;
                let name = &binding.names[&slot.id];
                writeln!(text, "        let __nested_{index} = self.{name}.encode()?;\n        if __nested_{index}.len() > {maximum} {{ return ::core::result::Result::Err({}); }}",fault(overflow)).unwrap();
                write!(sizes, " + __nested_{index}.len()").unwrap();
            }
            encoding => fixed += encoding.width(&graph.records).expect("validated layout").0,
        }
    }
    writeln!(
        text,
        "        let __size = {fixed}usize{sizes};\n        if __size > {max} {{ return ::core::result::Result::Err({}); }}\n        let mut __output = ::std::vec::Vec::with_capacity(__size);",
        fault(&graph.errors.encode_size)
    ).unwrap();
    for (index, slot) in slots.iter().enumerate() {
        let name = binding.names.get(&slot.id);
        let field = || format!("self.{}", name.expect("validated field"));
        match &slot.encoding {
            Encoding::Literal { bytes, .. } => {
                writeln!(text, "        __output.extend_from_slice(&{bytes:?});").unwrap();
            }
            Encoding::LengthPrefixedLiteral { bytes, .. } => {
                writeln!(
                    text,
                    "        __output.extend_from_slice(&{:?}u32.to_be_bytes());",
                    bytes.len()
                )
                .unwrap();
                writeln!(text, "        __output.extend_from_slice(&{bytes:?});").unwrap();
            }
            Encoding::UnsignedBe { .. } | Encoding::SignedBe { .. } => {
                writeln!(
                    text,
                    "        __output.extend_from_slice(&{}.to_be_bytes());",
                    field()
                )
                .unwrap();
            }
            Encoding::Bool { .. } => {
                writeln!(
                    text,
                    "        __output.push(::core::primitive::u8::from({}));",
                    field()
                )
                .unwrap();
            }
            Encoding::Nested { .. } => {
                writeln!(text, "        __output.extend_from_slice(&(__nested_{index}.len() as u32).to_be_bytes());\n        __output.extend_from_slice(&__nested_{index});").unwrap();
            }
            Encoding::Inline { id } => {
                for member in &graph.records[id].fields {
                    writeln!(
                        text,
                        "        __output.extend_from_slice(&{}.{}.to_be_bytes());",
                        field(),
                        binding.names[&member.id]
                    )
                    .unwrap();
                }
            }
            Encoding::Optional { payload, .. } => {
                writeln!(text, "        if let ::core::option::Option::Some(__value) = &{} {{\n            __output.push(1);", field()).unwrap();
                match payload {
                    Payload::UnsignedBe { .. } => writeln!(
                        text,
                        "            __output.extend_from_slice(&__value.to_be_bytes());"
                    )
                    .unwrap(),
                    Payload::Record { id } => {
                        for member in &graph.records[id].fields {
                            writeln!(text, "            __output.extend_from_slice(&__value.{}.to_be_bytes());", binding.names[&member.id]).unwrap();
                        }
                    }
                }
                writeln!(
                    text,
                    "        }} else {{\n            __output.push(0);\n        }}"
                )
                .unwrap();
            }
            Encoding::FixedBytes { .. } => {
                writeln!(text, "        __output.extend_from_slice(&{});", field()).unwrap();
            }
            Encoding::Bytes { .. } | Encoding::AsciiAtom { .. } => {
                let field = field();
                let view = match slot.encoding {
                    Encoding::AsciiAtom { .. } => format!("{field}.as_bytes()"),
                    _ => format!("&{field}"),
                };
                writeln!(text, "        __output.extend_from_slice(&({field}.len() as u32).to_be_bytes());\n        __output.extend_from_slice({view});").unwrap();
            }
            Encoding::EnumU8 { external, tags, .. } => {
                let native = &binding.externals[external];
                writeln!(text, "        __output.push(match &{} {{", field()).unwrap();
                for (variant, tag) in tags {
                    writeln!(
                        text,
                        "            {}::{} => {tag},",
                        native.path, native.variants[variant]
                    )
                    .unwrap();
                }
                writeln!(text, "        }});").unwrap();
            }
        }
    }
    if let Some(integrity) = &graph.integrity {
        hash_start(&mut text, integrity, binding);
        text.push_str("        __oddlang_hash.update(&__output);\n        __output.extend_from_slice(&__oddlang_hash.finalize());\n");
    }
    writeln!(text, "        ::core::result::Result::Ok(__output)\n    }}").unwrap();

    let reader = if graph.integrity.is_none() {
        "\n        let mut __rest = input;"
    } else {
        ""
    };
    writeln!(text, "    pub fn decode(input: &[u8]) -> ::core::result::Result<Self, {error}> {{\n        if input.len() > {max} {{ return ::core::result::Result::Err({}); }}{reader}", fault(&graph.errors.decode_size)).unwrap();
    if let Some(integrity) = &graph.integrity {
        writeln!(text,"        let __end = input.len().checked_sub(32).ok_or_else(|| {})?;\n        let (__payload, __checksum) = input.split_at(__end);",fault(&integrity.short)).unwrap();
        hash_start(&mut text, integrity, binding);
        writeln!(text,"        __oddlang_hash.update(__payload);\n        let __actual: [u8; 32] = __oddlang_hash.finalize().into();\n        if __actual != __checksum {{ return ::core::result::Result::Err({}); }}\n        let mut __rest = __payload;",fault(&integrity.mismatch)).unwrap();
    }
    let short = fault(&graph.errors.truncated);
    writeln!(text, "        fn __take<'a>(input: &mut &'a [u8], size: usize) -> ::core::result::Result<&'a [u8], {error}> {{\n            let ::core::option::Option::Some((head, tail)) = input.split_at_checked(size) else {{ return ::core::result::Result::Err({short}); }};\n            *input = tail;\n            ::core::result::Result::Ok(head)\n        }}").unwrap();
    if slots.iter().any(|slot| {
        matches!(
            slot.encoding,
            Encoding::AsciiAtom { .. }
                | Encoding::Bytes { .. }
                | Encoding::LengthPrefixedLiteral { .. }
                | Encoding::Nested { .. }
        )
    }) {
        writeln!(text, "        fn __length(input: &mut &[u8]) -> ::core::result::Result<usize, {error}> {{\n            ::core::result::Result::Ok(::core::primitive::u32::from_be_bytes(__take(input, 4)?.try_into().expect(\"fixed width\")) as usize)\n        }}").unwrap();
    }
    for (index, slot) in slots.iter().enumerate() {
        let variable = format!("__field_{index}");
        match &slot.encoding {
            Encoding::Literal {
                bytes,
                error,
                require_full,
            } => {
                if *require_full {
                    writeln!(text, "        if __take(&mut __rest,{})? != {bytes:?} {{ return ::core::result::Result::Err({}); }}",
                        bytes.len(), fault(error)).unwrap();
                } else {
                    writeln!(
                        text,
                        "        __rest = __rest.strip_prefix(&{bytes:?}).ok_or_else(|| {})?;",
                        fault(error)
                    )
                    .unwrap();
                }
            }
            Encoding::UnsignedBe {
                bits,
                minimum,
                maximum,
                read_error,
                ..
            } => {
                writeln!(
                    text,
                    "        let {variable} = {};",
                    read_integer(*bits, false)
                )
                .unwrap();
                if let Some(id) = read_error {
                    writeln!(text, "        if !({minimum}..={maximum}).contains(&{variable}) {{ return ::core::result::Result::Err({}); }}", fault(id)).unwrap();
                }
            }
            Encoding::SignedBe { bits } => {
                writeln!(
                    text,
                    "        let {variable} = {};",
                    read_integer(*bits, true)
                )
                .unwrap();
            }
            Encoding::Nested {
                record,
                maximum,
                overflow,
            } => {
                writeln!(text,"        let __size = __length(&mut __rest)?;\n        if __size > {maximum} {{ return ::core::result::Result::Err({}); }}",fault(overflow)).unwrap();
                writeln!(
                    text,
                    "        let {variable} = {}::decode(__take(&mut __rest, __size)?)?;",
                    binding.external_records[record].path
                )
                .unwrap();
            }
            Encoding::Inline { id } => {
                writeln!(text, "        let {variable} = {} {{", binding.names[id]).unwrap();
                for member in &graph.records[id].fields {
                    writeln!(
                        text,
                        "            {}: {},",
                        binding.names[&member.id],
                        read_integer(member.bits, member.signed)
                    )
                    .unwrap();
                }
                writeln!(text, "        }};").unwrap();
            }
            Encoding::Bool { error } => {
                writeln!(text, "        let {variable} = match __take(&mut __rest, 1)?[0] {{\n            0 => false,\n            1 => true,\n            _ => return ::core::result::Result::Err({}),\n        }};",fault(error)).unwrap();
            }
            Encoding::Optional { payload, error } => {
                writeln!(text, "        let {variable} = match __take(&mut __rest, 1)?[0] {{\n            0 => ::core::option::Option::None,").unwrap();
                match payload {
                    Payload::UnsignedBe { bits } => writeln!(
                        text,
                        "            1 => ::core::option::Option::Some({}),",
                        read_integer(*bits, false)
                    )
                    .unwrap(),
                    Payload::Record { id } => {
                        writeln!(
                            text,
                            "            1 => ::core::option::Option::Some({} {{",
                            binding.names[id]
                        )
                        .unwrap();
                        for member in &graph.records[id].fields {
                            writeln!(
                                text,
                                "                {}: {},",
                                binding.names[&member.id],
                                read_integer(member.bits, member.signed)
                            )
                            .unwrap();
                        }
                        writeln!(text, "            }}),").unwrap();
                    }
                }
                writeln!(
                    text,
                    "            _ => return ::core::result::Result::Err({}),\n        }};",
                    fault(error)
                )
                .unwrap();
            }
            Encoding::FixedBytes { size } => {
                writeln!(text, "        let {variable} = __take(&mut __rest, {size})?.try_into().expect(\"fixed width\");").unwrap();
            }
            Encoding::Bytes {
                maximum, overflow, ..
            }
            | Encoding::LengthPrefixedLiteral {
                maximum, overflow, ..
            }
            | Encoding::AsciiAtom {
                maximum, overflow, ..
            } => {
                writeln!(text, "        let __size = __length(&mut __rest)?;\n        if __size > {maximum} {{ return ::core::result::Result::Err({}); }}\n        let {variable} = __take(&mut __rest, __size)?;", fault(overflow)).unwrap();
                match &slot.encoding {
                    Encoding::LengthPrefixedLiteral { bytes, error, .. } => {
                        writeln!(
                            text,
                            "        if {variable} != {bytes:?} {{ return ::core::result::Result::Err({}); }}",
                            fault(error)
                        )
                        .unwrap();
                    }
                    Encoding::AsciiAtom {
                        minimum,
                        punctuation,
                        error,
                        ..
                    } => {
                        let rule = atom(&variable, *minimum, *maximum, punctuation);
                        writeln!(text, "        if !({rule}) {{ return ::core::result::Result::Err({}); }}\n        let {variable} = ::std::str::from_utf8({variable}).expect(\"validated ASCII\").to_owned();", fault(error)).unwrap();
                    }
                    _ => writeln!(text, "        let {variable} = {variable}.to_vec();").unwrap(),
                }
            }
            Encoding::EnumU8 {
                external,
                tags,
                error,
            } => {
                let native = &binding.externals[external];
                writeln!(
                    text,
                    "        let {variable} = match __take(&mut __rest, 1)?[0] {{"
                )
                .unwrap();
                for (variant, tag) in tags {
                    writeln!(
                        text,
                        "            {tag} => {}::{},",
                        native.path, native.variants[variant]
                    )
                    .unwrap();
                }
                if tags.len() < 256 {
                    writeln!(
                        text,
                        "            _ => return ::core::result::Result::Err({}),",
                        fault(error)
                    )
                    .unwrap();
                }
                writeln!(text, "        }};").unwrap();
            }
        }
    }
    writeln!(
        text,
        "        if !__rest.is_empty() {{ return ::core::result::Result::Err({}); }}\n        let __value = Self {{",
        fault(&graph.errors.trailing)
    )
    .unwrap();
    for (index, slot) in slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.encoding.field())
    {
        writeln!(
            text,
            "            {}: __field_{index},",
            binding.names[&slot.id]
        )
        .unwrap();
    }
    writeln!(
        text,
        "        }};\n        __value.validate()?;\n        ::core::result::Result::Ok(__value)\n    }}"
    )
    .unwrap();
    extension::render_views(&mut text, graph, binding);
    text.push_str("}\n");
    text
}

fn hash_start(text: &mut String, integrity: &super::Integrity, binding: &Binding) {
    match (
        &integrity.algorithm,
        binding
            .integrity
            .as_ref()
            .expect("validated integrity binding"),
    ) {
        (super::composite::Algorithm::Sha256, super::IntegrityBinding::Sha2) => {
            crate::wire_schema::sha256_begin(text, &integrity.domain)
        }
    }
}

fn read_integer(bits: u8, signed: bool) -> String {
    let ty = extension::integer(bits, signed);
    format!(
        "{ty}::from_be_bytes(__take(&mut __rest, {})?.try_into().expect(\"fixed width\"))",
        bits / 8
    )
}

fn atom(
    value: &str,
    minimum: u32,
    maximum: u32,
    punctuation: &std::collections::BTreeSet<u8>,
) -> String {
    let allowed: Vec<_> = punctuation.iter().copied().collect();
    format!(
        "({minimum}..={maximum}).contains(&{value}.len()) && {value}.iter().all(|byte| byte.is_ascii_alphanumeric() || {allowed:?}.contains(byte))"
    )
}
