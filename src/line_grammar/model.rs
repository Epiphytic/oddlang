use super::{
    constraints::Constraint,
    records::{RecordBinding, RecordResult},
};
use crate::{
    native_package::ident, shared_declarations::unique_map, text_grammar::Grammar, wire_schema::iri,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Result<T> = std::result::Result<T, String>;
pub(super) type Captures<'a> = BTreeMap<&'a str, Kind>;
pub(super) type Grammars<'a> = BTreeMap<&'a str, usize>;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_result: Option<RecordResult>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "format", deny_unknown_fields)]
pub(super) enum Binding {
    #[serde(rename = "oddlang-line-captures-rust-v1")]
    Captures(CaptureBinding),
    #[serde(rename = "oddlang-line-record-rust-v1")]
    Records(RecordBinding),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CaptureBinding {
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

fn lexical(s: &str, forbidden: &[u8]) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|byte| byte.is_ascii_graphic() && !forbidden.contains(&byte))
}

struct LexicalRules<'a, 'g> {
    forbidden: &'g [u8],
    grammars: &'g Grammars<'a>,
    referenced: BTreeSet<&'a str>,
}

impl LexicalRules<'_, '_> {
    fn token(&self, text: &str) -> bool {
        lexical(text, self.forbidden)
    }
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
    fn text_choices(&self, id: &str) -> Option<&[String]> {
        match self {
            Self::Text {
                capture, choices, ..
            } if capture.as_deref() == Some(id) && !choices.is_empty() => Some(choices),
            Self::Split { left, right, .. } => {
                left.text_choices(id).or_else(|| right.text_choices(id))
            }
            _ => None,
        }
    }

    fn validate<'a>(
        &'a self,
        lexical: &mut LexicalRules<'a, '_>,
        captures: &mut Captures<'a>,
    ) -> Result<()> {
        match self {
            Self::Literal { text, capture } => {
                if !lexical.token(text) {
                    return Err("literal token must be nonempty graphical ASCII".into());
                }
                add(captures, capture.as_deref(), Kind::StaticText)?;
            }
            Self::Text {
                capture,
                choices,
                grammar,
            } => {
                if choices.iter().any(|choice| !lexical.token(choice))
                    || choices.iter().collect::<BTreeSet<_>>().len() != choices.len()
                {
                    return Err("invalid text token choices".into());
                }
                if let Some(grammar) = grammar.as_deref() {
                    if !lexical.grammars.contains_key(grammar) {
                        return Err("unresolved nested text grammar".into());
                    }
                    lexical.referenced.insert(grammar);
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
                if false_token == true_token
                    || !lexical.token(false_token)
                    || !lexical.token(true_token)
                {
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
                if !lexical.token(text) || *value > unsigned_maximum(*bits)? {
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
                if !lexical.token(delimiter) {
                    return Err("token split delimiter must be nonempty graphical ASCII".into());
                }
                add(captures, capture.as_deref(), Kind::Text)?;
                left.validate(lexical, captures)?;
                right.validate(lexical, captures)?;
            }
        }
        Ok(())
    }
}

impl Graph {
    pub(super) fn text_choices(&self, id: &str) -> Option<&[String]> {
        self.rows
            .iter()
            .flatten()
            .find_map(|token| token.text_choices(id))
    }

    pub(super) fn validate(&self, binding: &Binding) -> Result<(Captures<'_>, Grammars<'_>)> {
        iri(&self.id)?;
        if self.format != "oddlang-line-grammar-v1"
            || self.max_bytes == 0
            || self.rows.is_empty()
            || self.rows.iter().any(Vec::is_empty)
            || (self.forbidden_bytes.contains(&b' ') && self.rows.iter().any(|row| row.len() > 1))
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
            if grammar
                .lexical_parts()
                .any(|text| !lexical(text, &self.forbidden_bytes))
            {
                return Err(
                    "nested grammar prefixes and delimiters must be admissible token text".into(),
                );
            }
            if grammars.insert(grammar.id(), index).is_some() {
                return Err("duplicate nested text grammar identity".into());
            }
        }
        let mut lexical = LexicalRules {
            forbidden: &self.forbidden_bytes,
            grammars: &grammars,
            referenced: BTreeSet::new(),
        };
        let mut captures = BTreeMap::new();
        let mut constraints = self.constraints.iter().peekable();
        for (index, row) in self.rows.iter().enumerate() {
            for token in row {
                token.validate(&mut lexical, &mut captures)?;
            }
            while let Some(constraint) =
                constraints.next_if(|constraint| constraint.row() == index + 1)
            {
                constraint.validate(&captures)?;
            }
        }
        if constraints.next().is_some() {
            return Err(
                "constraints require nondecreasing row positions within the grammar".into(),
            );
        }
        if lexical.referenced.len() != grammars.len() {
            return Err("unused nested text grammar".into());
        }
        match (binding, &self.record_result) {
            (Binding::Captures(binding), None) => binding.validate(&captures)?,
            (Binding::Records(binding), Some(result)) => {
                result.validate(binding, &captures)?;
                binding.validate_support(self, &grammars)?;
            }
            _ => return Err("Rust overlay must match the grammar result form".into()),
        }
        Ok((captures, grammars))
    }
}

impl CaptureBinding {
    fn validate(&self, captures: &Captures<'_>) -> Result<()> {
        if !ident(&self.reader)
            || !ident(&self.record)
            || self.reader == self.record
            || self
                .capture_names
                .keys()
                .map(String::as_str)
                .ne(captures.keys().copied())
            || self.capture_names.values().any(|name| !ident(name))
            || self.capture_names.values().collect::<BTreeSet<_>>().len() != captures.len()
        {
            return Err("Rust overlay must assign distinct valid names for the parser, result and every capture".into());
        }
        Ok(())
    }
}
