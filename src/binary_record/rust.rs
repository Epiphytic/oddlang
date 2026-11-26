use super::{Binding, Encoding, Graph};
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
    writeln!(
        text,
        "#[derive(Clone, PartialEq, Eq)]\npub struct {name} {{"
    )
    .unwrap();
    for slot in slots.iter().filter(|slot| slot.encoding.field()) {
        let ty = match &slot.encoding {
            Encoding::UnsignedBe { bits, .. } => format!("::core::primitive::u{bits}"),
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
        let rule = match &slot.encoding {
            Encoding::UnsignedBe {
                minimum, maximum, ..
            } => {
                format!("({minimum}..={maximum}).contains(&self.{field})")
            }
            Encoding::Bytes {
                minimum, maximum, ..
            } => {
                format!("({minimum}..={maximum}).contains(&self.{field}.len())")
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
            "        if !({rule}) {{ return ::core::result::Result::Err({value_fault}); }}"
        )
        .unwrap();
    }
    writeln!(text, "        ::core::result::Result::Ok(())\n    }}").unwrap();

    writeln!(text, "    pub fn encode(&self) -> ::core::result::Result<::std::vec::Vec<::core::primitive::u8>, {error}> {{\n        self.validate()?;").unwrap();
    let mut fixed = 0u64;
    let mut sizes = String::new();
    for slot in slots {
        match &slot.encoding {
            Encoding::Bytes { .. } | Encoding::AsciiAtom { .. } => {
                fixed += 4;
                write!(sizes, " + self.{}.len()", binding.names[&slot.id]).unwrap();
            }
            encoding => fixed += encoding.width().expect("validated layout").0,
        }
    }
    writeln!(
        text,
        "        let __size = {fixed}usize{sizes};\n        if __size > {max} {{ return ::core::result::Result::Err({}); }}\n        let mut __output = ::std::vec::Vec::with_capacity(__size);",
        fault(&graph.errors.encode_size)
    ).unwrap();
    for slot in slots {
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
            Encoding::UnsignedBe { .. } => {
                writeln!(
                    text,
                    "        __output.extend_from_slice(&{}.to_be_bytes());",
                    field()
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
    writeln!(text, "        ::core::result::Result::Ok(__output)\n    }}").unwrap();

    writeln!(text, "    pub fn decode(input: &[u8]) -> ::core::result::Result<Self, {error}> {{\n        if input.len() > {max} {{ return ::core::result::Result::Err({}); }}\n        let mut __rest = input;", fault(&graph.errors.decode_size)).unwrap();
    let short = fault(&graph.errors.truncated);
    writeln!(text, "        fn __take<'a>(input: &mut &'a [u8], size: usize) -> ::core::result::Result<&'a [u8], {error}> {{\n            let ::core::option::Option::Some((head, tail)) = input.split_at_checked(size) else {{ return ::core::result::Result::Err({short}); }};\n            *input = tail;\n            ::core::result::Result::Ok(head)\n        }}").unwrap();
    if slots.iter().any(|slot| {
        matches!(
            slot.encoding,
            Encoding::AsciiAtom { .. }
                | Encoding::Bytes { .. }
                | Encoding::LengthPrefixedLiteral { .. }
        )
    }) {
        writeln!(text, "        fn __length(input: &mut &[u8]) -> ::core::result::Result<usize, {error}> {{\n            ::core::result::Result::Ok(::core::primitive::u32::from_be_bytes(__take(input, 4)?.try_into().expect(\"fixed width\")) as usize)\n        }}").unwrap();
    }
    for (index, slot) in slots.iter().enumerate() {
        let variable = format!("__field_{index}");
        match &slot.encoding {
            Encoding::Literal { bytes, error } => {
                writeln!(
                    text,
                    "        __rest = __rest.strip_prefix(&{bytes:?}).ok_or_else(|| {})?;",
                    fault(error)
                )
                .unwrap();
            }
            Encoding::UnsignedBe { bits, .. } => {
                writeln!(text, "        let {variable} = ::core::primitive::u{bits}::from_be_bytes(__take(&mut __rest, {})?.try_into().expect(\"fixed width\"));", bits / 8).unwrap();
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
        "        }};\n        __value.validate()?;\n        ::core::result::Result::Ok(__value)\n    }}\n}}"
    )
    .unwrap();
    text
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
