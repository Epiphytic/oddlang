use super::{Binding, Envelope, FieldKind, Guard};
use std::fmt::Write;

pub(super) fn render(envelope: &Envelope, binding: &Binding) -> String {
    let Binding {
        codec_type,
        limits_type,
        error_type,
        encode,
        decode,
        constants,
        ..
    } = binding;
    let raw = &constants.raw_bytes;
    let wire = &constants.wire_bytes;
    let line = &constants.record_line_bytes;
    let [raw_bytes, wire_bytes, line_bytes] = envelope.limits;
    let [size_error, json_error, literal_error, encoding_error] = envelope
        .error_ids
        .each_ref()
        .map(|id| format!("{error_type}::{}", binding.variants[id]));
    let literal = envelope
        .fields
        .iter()
        .find_map(|field| match field.kind {
            FieldKind::UnsignedLiteral { value, .. } => Some(value),
            FieldKind::CanonicalBase64 => None,
        })
        .expect("a validated unsigned literal");
    let mut source = format!(
        r#"#[derive(Clone, Copy, Debug, Default)]
pub struct {limits_type};

impl {limits_type} {{
    pub const {raw}: ::core::primitive::usize = {raw_bytes};
    pub const {wire}: ::core::primitive::usize = {wire_bytes};
    pub const {line}: ::core::primitive::usize = {line_bytes};
}}

#[derive(Debug, PartialEq, Eq)]
pub enum {error_type} {{
"#
    );
    for variant in binding.variants.values() {
        writeln!(source, "    {variant},").unwrap();
    }
    writeln!(
        source,
        r#"}}

impl ::core::fmt::Display for {error_type} {{
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{
        f.write_str(match self {{"#
    )
    .unwrap();
    for (id, message) in &envelope.errors {
        writeln!(
            source,
            "            Self::{} => {message:?},",
            binding.variants[id]
        )
        .unwrap();
    }
    writeln!(
        source,
        r#"        }})
    }}
}}

impl ::core::error::Error for {error_type} {{}}

mod oddlang_json_record {{
    #[derive(::serde::Serialize, ::serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Record {{"#
    )
    .unwrap();
    for field in &envelope.fields {
        let (member, ty) = match field.kind {
            FieldKind::UnsignedLiteral { bits, .. } => {
                ("literal", format!("::core::primitive::u{bits}"))
            }
            FieldKind::CanonicalBase64 => ("binary", "::std::string::String".into()),
        };
        writeln!(
            source,
            "        #[serde(rename = {:?})]\n        pub(super) {member}: {ty},",
            field.name
        )
        .unwrap();
    }
    writeln!(
        source,
        r#"    }}
}}

pub struct {codec_type};

impl {codec_type} {{
    pub fn {encode}(bytes: &[::core::primitive::u8]) -> ::core::result::Result<::std::vec::Vec<::core::primitive::u8>, {error_type}> {{
        use ::base64::Engine as _;"#
    ).unwrap();
    for guard in &envelope.encode {
        match guard {
            Guard::RawSize => reject(
                &mut source,
                &format!("bytes.len() > {limits_type}::{raw}"),
                &size_error,
            ),
            Guard::Json => writeln!(
                source,
                r#"        let frame = ::serde_json::to_vec(&oddlang_json_record::Record {{
            literal: {literal},
            binary: ::base64::engine::general_purpose::STANDARD.encode(bytes),
        }}).map_err(|_| {json_error})?;"#
            )
            .unwrap(),
            Guard::WireSize => reject(
                &mut source,
                &format!("frame.len() > {limits_type}::{wire}"),
                &size_error,
            ),
            _ => unreachable!("validated encoding order"),
        }
    }
    writeln!(
        source,
        r#"        ::core::result::Result::Ok(frame)
    }}

    pub fn {decode}(frame: &[::core::primitive::u8]) -> ::core::result::Result<::std::vec::Vec<::core::primitive::u8>, {error_type}> {{
        use ::base64::Engine as _;"#
    ).unwrap();
    for guard in &envelope.decode {
        match guard {
            Guard::WireSize => reject(&mut source, &format!("frame.len() > {limits_type}::{wire}"), &size_error),
            Guard::Json => {
                if !envelope.accept_tuple {
                    reject(
                        &mut source,
                        r"frame.iter().copied().find(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')) != ::core::option::Option::Some(b'{')",
                        &json_error,
                    );
                }
                writeln!(
                    source,
                    r#"        let record: oddlang_json_record::Record = ::serde_json::from_slice(frame)
            .map_err(|_| {json_error})?;"#
                ).unwrap();
            }
            Guard::Literal => reject(&mut source, &format!("record.literal != {literal}"), &literal_error),
            Guard::EncodedSize => reject(&mut source, &format!("record.binary.len() > {limits_type}::{raw}.div_ceil(3) * 4"), &size_error),
            Guard::Base64 => writeln!(
                source,
                r#"        let bytes = ::base64::engine::general_purpose::STANDARD.decode(&record.binary)
            .map_err(|_| {encoding_error})?;"#
            ).unwrap(),
            Guard::RawSize => reject(&mut source, &format!("bytes.len() > {limits_type}::{raw}"), &size_error),
            Guard::Canonical => reject(&mut source, "::base64::engine::general_purpose::STANDARD.encode(&bytes) != record.binary", &encoding_error),
        }
    }
    source.push_str("        ::core::result::Result::Ok(bytes)\n    }\n}\n");
    source
}

fn reject(source: &mut String, condition: &str, error: &str) {
    writeln!(
        source,
        "        if {condition} {{\n            return ::core::result::Result::Err({error});\n        }}"
    ).unwrap();
}
