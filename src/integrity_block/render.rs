use super::{Graph, Overlay, Role};
use std::fmt::Write;

const CORE: &str = "__oddlang_integrity_block";
const ERROR: &str = "__OddlangIntegrityFailure";

pub(super) fn project(graph: &Graph, binding: &Overlay) -> String {
    let block = &binding.block_type;
    let mode = &binding.representation_type;
    let operations = &binding.operations;
    let identity = &binding.variants.identity;
    let encoded = binding.fields.get(&Role::Payload).unwrap();
    let size = binding.fields.get(&Role::RawLength).unwrap();
    let digest = binding.fields.get(&Role::Digest).unwrap();
    let position = binding.fields.get(&Role::Offset).unwrap();
    let representation = binding.fields.get(&Role::Representation).unwrap();
    let helper = binding.payload_helper.as_ref();
    let module = helper.map_or("__oddlang_integrity_payload", |helper| {
        helper.module.as_str()
    });
    let decode_field = helper.map_or("decode_field", |helper| helper.decode.as_str());
    let visibility = if helper.is_some() { "pub(crate) " } else { "" };

    let mut text = format!(
        r#"type {ERROR} = {error_type};

mod {CORE} {{
const RAW_MIN: usize = {raw_min};
const RAW_MAX: usize = {raw_max};
const ENCODED_MIN: usize = {encoded_min};
pub(super) const ENCODED_MAX: usize = {encoded_max};
const OFFSET_MAX: u64 = {offset_max};
const HEX_SYNTAX: &str = {hex_syntax:?};
const CHECKS: &[Check] = &[
"#,
        error_type = binding.error_type,
        raw_min = graph.bounds.raw_minimum,
        raw_max = graph.bounds.raw_maximum,
        encoded_min = graph.bounds.encoded_minimum,
        encoded_max = graph.bounds.encoded_maximum,
        offset_max = graph.bounds.offset_maximum,
        hex_syntax = graph.wire.digest_syntax_message,
    );
    for check in &graph.validate_order {
        writeln!(text, "    Check::{check:?},").unwrap();
    }
    text.push_str(
        "];
",
    );
    text.push_str(include_str!("runtime.rs"));
    if graph.representations.compressed.is_some() {
        text.push_str(include_str!("zlib.rs"));
    }
    writeln!(
        text,
        r#"}}

fn __oddlang_integrity_fault(fault: {CORE}::Fault) -> {ERROR} {{
    match fault {{"#
    )
    .unwrap();
    for (role, id) in graph.errors.roles() {
        let constructor = &binding.error_constructors[id];
        let variant = &constructor.variant;
        match &constructor.argument {
            Some(argument) => writeln!(
                text,
                "        {CORE}::Fault::{role} => {ERROR}::{variant}({argument:?}),"
            ),
            None => writeln!(text, "        {CORE}::Fault::{role} => {ERROR}::{variant},"),
        }
        .unwrap();
    }
    writeln!(
        text,
        r#"    }}
}}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
pub enum {mode} {{
    #[serde(rename = {:?})]
    {identity},"#,
        graph.representations.identity
    )
    .unwrap();
    if let Some((policy, variant)) = graph
        .representations
        .compressed
        .as_ref()
        .zip(binding.variants.compressed.as_ref())
    {
        writeln!(
            text,
            "    #[serde(rename = {:?})]
    {variant},",
            policy.wire_name
        )
        .unwrap();
    }
    text.push_str(
        "}

#[derive(Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
",
    );
    writeln!(
        text,
        "#[serde(deny_unknown_fields)]
pub struct {block} {{"
    )
    .unwrap();
    for field in &graph.fields {
        write!(text, "    #[serde(rename = {:?}", field.wire_name).unwrap();
        let kind = match field.role {
            Role::Offset => "u64",
            Role::Representation => mode,
            Role::RawLength => "u32",
            Role::Digest => {
                write!(text, r#", with = "{CORE}::hex""#).unwrap();
                "[u8; 32]"
            }
            Role::Payload => {
                write!(text, r#", with = "{module}""#).unwrap();
                "::std::vec::Vec<u8>"
            }
        };
        writeln!(
            text,
            ")]
    pub {}: {kind},",
            binding.fields[&field.role]
        )
        .unwrap();
    }
    let compressed = binding.variants.compressed.as_ref();
    let compressed_check = compressed.map_or("false".to_owned(), |name| {
        format!("self.{representation} == {mode}::{name}")
    });
    writeln!(
        text,
        r#"}}

impl {block} {{
    fn __oddlang_frame(&self) -> {CORE}::Frame<'_> {{
        {CORE}::Frame {{
            offset: self.{position},
            size: self.{size},
            compressed: {compressed_check},
            digest: &self.{digest},
            payload: &self.{encoded},
        }}
    }}

    pub fn {end}(&self) -> ::core::result::Result<u64, {ERROR}> {{
        {CORE}::end(self.{position}, self.{size}).map_err(__oddlang_integrity_fault)
    }}

    pub fn {validate}(&self) -> ::core::result::Result<(), {ERROR}> {{
        {CORE}::validate(&self.__oddlang_frame()).map_err(__oddlang_integrity_fault)
    }}
}}"#,
        end = operations.end,
        validate = operations.validate,
    )
    .unwrap();
    let negotiation = if compressed.is_some() {
        "negotiated"
    } else {
        "_negotiated"
    };
    let mutable = if compressed.is_some() { "mut " } else { "" };
    writeln!(
        text,
        r#"
pub fn {encode}(offset: u64, raw: &[u8], {negotiation}: {mode}) -> ::core::result::Result<{block}, {ERROR}> {{
    {CORE}::raw_size(raw.len()).map_err(__oddlang_integrity_fault)?;
    let {mutable}block = {block} {{
        {position}: offset,
        {representation}: {mode}::{identity},
        {size}: raw.len() as u32,
        {digest}: {CORE}::checksum(raw),
        {encoded}: raw.to_vec(),
    }};
    block.{validate}()?;"#,
        encode = operations.encode,
        validate = operations.validate,
    )
    .unwrap();
    if let Some(variant) = compressed {
        writeln!(
            text,
            r#"    if negotiated == {mode}::{variant} {{
        let candidate = {CORE}::compress(raw).map_err(__oddlang_integrity_fault)?;
        if candidate.len() < raw.len() && {CORE}::encoded_size(candidate.len()) {{
            block.{representation} = {mode}::{variant};
            block.{encoded} = candidate;
        }}
    }}"#
        )
        .unwrap();
    }
    writeln!(
        text,
        r#"    ::core::result::Result::Ok(block)
}}

pub fn {decode}(block: &{block}, {negotiation}: {mode}) -> ::core::result::Result<::std::vec::Vec<u8>, {ERROR}> {{
    let frame = block.__oddlang_frame();
    {CORE}::validate(&frame).map_err(__oddlang_integrity_fault)?;"#,
        decode = operations.decode,
    )
    .unwrap();
    if let Some(variant) = compressed {
        writeln!(
            text,
            r#"    let raw = match block.{representation} {{
        {mode}::{identity} => frame.payload.to_vec(),
        {mode}::{variant} => {CORE}::decompress(&frame, negotiated == {mode}::{variant})
            .map_err(__oddlang_integrity_fault)?,
    }};"#
        )
        .unwrap();
    } else {
        text.push_str(
            "    let raw = frame.payload.to_vec();
",
        );
    }
    writeln!(
        text,
        r#"    {CORE}::verify_checksum(&frame, &raw).map_err(__oddlang_integrity_fault)?;
    ::core::result::Result::Ok(raw)
}}

{visibility}mod {module} {{
    use ::base64::{{Engine as _, engine::general_purpose::STANDARD}};
    use ::serde::{{Deserialize, Deserializer, Serializer, de::Error as _}};

    pub fn serialize<S: Serializer>(bytes: &[u8], writer: S) -> Result<S::Ok, S::Error> {{
        writer.serialize_str(&STANDARD.encode(bytes))
    }}

    pub fn deserialize<'de, D: Deserializer<'de>>(reader: D) -> Result<Vec<u8>, D::Error> {{
        let encoded = String::deserialize(reader)?;"#
    )
    .unwrap();
    if graph.error_messages.is_empty() && helper.is_some() {
        writeln!(
            text,
            "        {decode_field}(&encoded, super::{CORE}::ENCODED_MAX).map_err(D::Error::custom)"
        )
        .unwrap();
    } else {
        writeln!(
            text,
            r#"        super::{CORE}::decode_payload(&encoded, super::{CORE}::ENCODED_MAX)
            .map_err(|fault| match fault {{"#
        )
        .unwrap();
        for (role, id) in graph.errors.roles() {
            if let Some(message) = graph.error_messages.get(id) {
                writeln!(
                    text,
                    "                super::{CORE}::Fault::{role} => D::Error::custom({message:?}),"
                )
                .unwrap();
            }
        }
        writeln!(
            text,
            r#"                other => D::Error::custom(super::__oddlang_integrity_fault(other)),
            }})"#
        )
        .unwrap();
    }
    text.push_str("    }\n");
    if helper.is_some() {
        if !graph.error_messages.is_empty() {
            text.push_str("    #[allow(dead_code)]\n");
        }
        writeln!(text, r#"    pub(crate) fn {decode_field}(encoded: &str, limit: usize) -> Result<Vec<u8>, super::{ERROR}> {{
        super::{CORE}::decode_payload(encoded, limit).map_err(super::__oddlang_integrity_fault)
    }}"#).unwrap();
    }
    text.push_str("}\n");
    text
}
