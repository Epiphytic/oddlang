use crate::{
    native_package::ident, shared_declarations::unique_map, text_grammar::Grammar, wire_schema::iri,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Result<T> = std::result::Result<T, String>;
pub(super) type Captures<'a> = BTreeMap<&'a str, Kind>;
type Grammars<'a> = BTreeMap<&'a str, usize>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Graph {
    pub format: String,
    pub id: String,
    pub max_bytes: u32,
    pub forbidden_bytes: Vec<u8>,
    #[serde(default)]
    pub text_grammars: Vec<Grammar>,
    pub rows: Vec<Vec<Token>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Binding {
    pub format: String,
    pub reader: String,
    pub record: String,
    #[serde(deserialize_with = "unique_map")]
    pub capture_names: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Kind {
    Text,
    StaticText,
    Decimal(u8),
    Bytes(u16),
    Boolean,
}

impl Kind {
    pub(super) fn rust_type(self) -> String {
        match self {
            Self::Text => "&'input ::core::primitive::str".into(),
            Self::StaticText => "&'static ::core::primitive::str".into(),
            Self::Decimal(bits) => format!("::core::primitive::u{bits}"),
            Self::Bytes(bytes) => format!("[::core::primitive::u8; {bytes}]"),
            Self::Boolean => "::core::primitive::bool".into(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Token {
    Literal {
        text: String,
        capture: Option<String>,
    },
    Text {
        capture: Option<String>,
        #[serde(default)]
        choices: Vec<String>,
        grammar: Option<String>,
    },
    Decimal {
        capture: Option<String>,
        bits: u8,
        minimum: u64,
        maximum: Option<u64>,
    },
    Radix16 {
        capture: String,
        digits: u16,
        decode: bool,
    },
    Boolean {
        capture: String,
        false_token: String,
        true_token: String,
    },
    IntegerToken {
        capture: String,
        text: String,
        bits: u8,
        value: u64,
    },
    Split {
        capture: Option<String>,
        delimiter: String,
        left: Box<Token>,
        right: Box<Token>,
    },
}

pub(super) fn unsigned_maximum(bits: u8) -> Result<u64> {
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err("expected an unsigned width of 8, 16, 32 or 64 bits".into());
    }
    Ok(u64::MAX >> (64 - bits))
}

fn lexical(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|byte| byte.is_ascii_graphic())
}

fn add<'a>(captures: &mut Captures<'a>, id: Option<&'a str>, kind: Kind) -> Result<()> {
    if let Some(id) = id {
        if id.is_empty() || captures.insert(id, kind).is_some() {
            return Err(format!("empty or duplicate capture: {id}"));
        }
    }
    Ok(())
}

impl Token {
    fn validate<'a>(&'a self, grammars: &Grammars<'_>, captures: &mut Captures<'a>) -> Result<()> {
        match self {
            Self::Literal { text, capture } => {
                if !lexical(text) {
                    return Err("literal token must be nonempty graphical ASCII".into());
                }
                add(captures, capture.as_deref(), Kind::StaticText)?;
            }
            Self::Text {
                capture,
                choices,
                grammar,
            } => {
                if choices.iter().any(|choice| !lexical(choice))
                    || choices.iter().collect::<BTreeSet<_>>().len() != choices.len()
                    || grammar
                        .as_deref()
                        .is_some_and(|id| !grammars.contains_key(id))
                {
                    return Err("invalid text token choices or unresolved text grammar".into());
                }
                add(captures, capture.as_deref(), Kind::Text)?;
            }
            Self::Decimal {
                capture,
                bits,
                minimum,
                maximum,
            } => {
                let native = unsigned_maximum(*bits)?;
                let max = maximum.unwrap_or(native);
                if *minimum > max || max > native {
                    return Err("decimal bounds contradict its unsigned width".into());
                }
                add(captures, capture.as_deref(), Kind::Decimal(*bits))?;
            }
            Self::Radix16 {
                capture,
                digits,
                decode,
            } => {
                if *digits == 0 || (*decode && !digits.is_multiple_of(2)) {
                    return Err("invalid canonical hexadecimal extent".into());
                }
                add(
                    captures,
                    Some(capture),
                    if *decode {
                        Kind::Bytes(*digits / 2)
                    } else {
                        Kind::Text
                    },
                )?;
            }
            Self::Boolean {
                capture,
                false_token,
                true_token,
            } => {
                if false_token == true_token || !lexical(false_token) || !lexical(true_token) {
                    return Err("boolean encoding requires distinct graphical ASCII tokens".into());
                }
                add(captures, Some(capture), Kind::Boolean)?;
            }
            Self::IntegerToken {
                capture,
                text,
                bits,
                value,
            } => {
                if !lexical(text) || *value > unsigned_maximum(*bits)? {
                    return Err("invalid integer-valued literal token".into());
                }
                add(captures, Some(capture), Kind::Decimal(*bits))?;
            }
            Self::Split {
                capture,
                delimiter,
                left,
                right,
            } => {
                if !lexical(delimiter) {
                    return Err("token split delimiter must be nonempty graphical ASCII".into());
                }
                add(captures, capture.as_deref(), Kind::Text)?;
                left.validate(grammars, captures)?;
                right.validate(grammars, captures)?;
            }
        }
        Ok(())
    }
}

impl Graph {
    pub(super) fn validate(&self, binding: &Binding) -> Result<(Captures<'_>, Grammars<'_>)> {
        iri(&self.id)?;
        if self.format != "oddlang-line-grammar-v1"
            || self.max_bytes == 0
            || self.rows.is_empty()
            || self.rows.iter().any(Vec::is_empty)
            || self
                .forbidden_bytes
                .iter()
                .any(|byte| !byte.is_ascii() || *byte == b'\n')
            || self.forbidden_bytes.iter().collect::<BTreeSet<_>>().len()
                != self.forbidden_bytes.len()
        {
            return Err("invalid bounded ASCII line framing".into());
        }
        let mut grammars = BTreeMap::new();
        for (index, grammar) in self.text_grammars.iter().enumerate() {
            grammar.validate()?;
            if grammars.insert(grammar.id(), index).is_some() {
                return Err("duplicate nested text grammar identity".into());
            }
        }
        let mut captures = BTreeMap::new();
        for row in &self.rows {
            for token in row {
                token.validate(&grammars, &mut captures)?;
            }
        }
        if binding.format != "oddlang-line-captures-rust-v1"
            || !ident(&binding.reader)
            || !ident(&binding.record)
            || binding.reader == binding.record
            || binding
                .capture_names
                .keys()
                .map(String::as_str)
                .ne(captures.keys().copied())
            || binding.capture_names.values().any(|name| !ident(name))
            || binding
                .capture_names
                .values()
                .collect::<BTreeSet<_>>()
                .len()
                != captures.len()
        {
            return Err("Rust overlay must assign distinct valid names for the parser, result and every capture".into());
        }
        Ok((captures, grammars))
    }
}
