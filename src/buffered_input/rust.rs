use super::*;
use crate::wire_schema::{digest, encode, symbol, type_symbol};
use serde_json::json;
use std::{fmt::Write, fs, path::Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Overlay {
    format: String,
    id: String,
    #[serde(rename = "type")]
    name: String,
    parameter: String,
    fields: recovery::FieldNames,
}

pub(crate) fn build_bytes(raw: &[u8], overlay: &[u8], out: &Path) -> Result<()> {
    let graph: Graph = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
    let native: Overlay = serde_json::from_slice(overlay).map_err(|e| e.to_string())?;
    graph.validate()?;
    let bindings = native.fields.entries();
    if native.id != graph.id
        || native.format != "oddlang-buffered-input-rust-v1"
        || !type_symbol(&native.name)
        || !symbol(&native.parameter)
        || native.parameter == native.name
        || reserved(&native.name)
        || reserved(&native.parameter)
        || bindings.iter().any(|(_, n)| !symbol(n))
        || bindings
            .iter()
            .map(|(_, n)| n)
            .collect::<BTreeSet<_>>()
            .len()
            != 4
    {
        return Err("invalid buffered-input Rust binding".into());
    }
    let name = native.name;
    let param = native.parameter;
    let n = graph.capacity;
    let fields = native.fields;
    let mut source = format!("struct {name}<{param}> {{\n");
    for role in graph.field_order {
        let (field, ty) = match role {
            Role::Input => (&fields.input, param.clone()),
            Role::Storage => (&fields.storage, format!("::zeroize::Zeroizing<[u8; {n}]>")),
            Role::Begin => (&fields.begin, "usize".into()),
            Role::End => (&fields.end, "usize".into()),
        };
        writeln!(source, "    {field}: {ty},").unwrap();
    }
    let input = fields.input;
    let storage = fields.storage;
    let begin = fields.begin;
    let end = fields.end;
    let pre_read = match graph.transitions.empty_read {
        EmptyRead::RefillIfExhausted => "",
        EmptyRead::ReturnZero => "        if output.is_empty() { return Ok(0); }\n",
    };
    let on_error = match graph.transitions.refill_error {
        RefillError::ResetBeginRetainEnd => String::new(),
        RefillError::DiscardWindow => format!("            self.{end} = 0;\n"),
    };
    let retire = match graph.transitions.consumed_bytes {
        ConsumedBytes::Erase => format!(
            "        ::zeroize::Zeroize::zeroize(&mut self.{storage}[self.{begin}..next]);\n"
        ),
        ConsumedBytes::Retain => String::new(),
    };
    write!(source,r#"}}
impl<{param}: ::std::io::Read> ::std::io::BufRead for {name}<{param}> {{
    fn fill_buf(&mut self) -> ::std::io::Result<&[u8]> {{
        if self.{begin} == self.{end} {{
            self.{begin} = 0;
{on_error}            let filled = ::std::io::Read::read(&mut self.{input}, &mut self.{storage}[..])?;
            self.{end} = filled;
        }}
        Ok(&self.{storage}[self.{begin}..self.{end}])
    }}
    fn consume(&mut self, requested: usize) {{
        let remaining = self.{end} - self.{begin};
        let next = self.{begin} + ::core::cmp::min(remaining, requested);
{retire}        self.{begin} = next;
    }}
}}
impl<{param}: ::std::io::Read> ::std::io::Read for {name}<{param}> {{
    fn read(&mut self, output: &mut [u8]) -> ::std::io::Result<usize> {{
{pre_read}        let view = ::std::io::BufRead::fill_buf(self)?;
        let copied = ::core::cmp::min(output.len(), view.len());
        output[..copied].copy_from_slice(&view[..copied]);
        ::std::io::BufRead::consume(self, copied);
        Ok(copied)
    }}
}}
"#).unwrap();
    fs::create_dir(out).map_err(|e| e.to_string())?;
    fs::write(out.join("buffer.rs"), &source).map_err(|e| e.to_string())?;
    fs::write(
        out.join("projection.lock"),
        encode(&json!({
            "format":"oddlang-buffered-input-rust-lock-v1","graphSha256":digest(raw),
            "overlaySha256":digest(overlay),"sourceSha256":digest(source.as_bytes())
        }))?,
    )
    .map_err(|e| e.to_string())
}
