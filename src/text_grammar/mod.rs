//! Exact literal prefix and decimal text grammars.
mod recover;

pub use recover::recover;

use crate::{
    native_package::publish,
    wire_schema::{digest, encode, iri, json},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeSet, fs, path::Path};

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-text-grammar-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grammar {
    format: String,
    id: String,
    prefixes: Vec<String>,
    body: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suffix: Option<Suffix>,
    consumption: Consumption,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Consumption {
    EntireInput,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Suffix {
    delimiter: String,
    body: Decimal,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Decimal {
    notation: Notation,
    bits: u8,
    minimum: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Notation {
    CanonicalAsciiDecimal,
}

impl Decimal {
    fn bounds(&self) -> Result<(u64, u64)> {
        if !matches!(self.bits, 8 | 16 | 32 | 64) {
            return Err("unsupported unsigned decimal width".into());
        }
        let maximum = u64::MAX >> (64 - self.bits);
        if self.minimum > maximum {
            return Err("decimal minimum exceeds its unsigned width".into());
        }
        Ok((self.minimum, maximum))
    }
}

impl Grammar {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn validate(&self) -> Result<()> {
        iri(&self.id)?;
        if self.format != FORMAT
            || self.prefixes.is_empty()
            || self.prefixes.iter().any(String::is_empty)
            || self.prefixes.iter().collect::<BTreeSet<_>>().len() != self.prefixes.len()
        {
            return Err("expected a text grammar with distinct nonempty literal prefixes".into());
        }
        self.body.bounds()?;
        if let Some(suffix) = &self.suffix {
            if suffix.delimiter.is_empty() {
                return Err("optional decimal suffix requires a nonempty delimiter".into());
            }
            suffix.body.bounds()?;
        }
        Ok(())
    }
}

const RUST: &str = r#"
pub fn accepts(value: &str) -> bool {
    let Some(rest) = PREFIXES.iter().find_map(|prefix| value.strip_prefix(*prefix)) else {
        return false;
    };
    if let Some((delimiter, bound)) = SUFFIX {
        if let Some((first, second)) = rest.split_once(delimiter) {
            return decimal_in(first, BODY) && decimal_in(second, bound);
        }
    }
    decimal_in(rest, BODY)
}

fn decimal_in(text: &str, (minimum, maximum): (u64, u64)) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || (bytes[0] == b'0' && bytes.len() > 1) {
        return false;
    }
    let parsed = bytes.iter().try_fold(0_u64, |value, &octet| {
        if !octet.is_ascii_digit() {
            return None;
        }
        value.checked_mul(10)?.checked_add(u64::from(octet - b'0'))
    });
    parsed.is_some_and(|value| (minimum..=maximum).contains(&value))
}
"#;

impl Grammar {
    pub(crate) fn render(&self) -> Result<String> {
        self.validate()?;
        let body = self.body.bounds()?;
        let suffix = if let Some(suffix) = &self.suffix {
            format!(
                "Some(({:?}, {:?}))",
                suffix.delimiter,
                suffix.body.bounds()?
            )
        } else {
            "None".into()
        };
        Ok(format!(
            "const PREFIXES: &[&str] = &{:?};\nconst BODY: (u64, u64) = {body:?};\nconst SUFFIX: Option<(&str, (u64, u64))> = {suffix};\n{RUST}",
            self.prefixes
        ))
    }
}

pub fn build(path: &Path, output: &Path) -> Result<()> {
    let grammar: Grammar = json(path)?;
    let source = grammar.render()?;
    let obligations = encode(&json!({
        "declaration": grammar.id,
        "role": "text-grammar",
        "expected": grammar,
        "qualification": "independent-witness-required",
    }))?;
    let lock = encode(&json!({
        "format": "oddlang-text-grammar-lock-v1",
        "target": "rust",
        "graphSha256": digest(&encode(&grammar)?),
        "sourceSha256": digest(source.as_bytes()),
        "obligationsSha256": digest(&obligations),
    }))?;
    write(
        output,
        &[
            ("text.rs", source.as_bytes()),
            ("obligations.json", &obligations),
            ("projection.lock", &lock),
        ],
    )
}

fn write(output: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|err| err.to_string())?;
    for (name, bytes) in files {
        fs::write(stage.path().join(name), bytes).map_err(|err| err.to_string())?;
    }
    publish(stage.path(), output)
}
