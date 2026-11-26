use super::model::{Captures, Kind, Result, unsigned_maximum};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Constraint {
    AllowedTuples {
        after_row: usize,
        captures: Vec<String>,
        allowed: Vec<Vec<Scalar>>,
    },
    CheckedSum {
        after_row: usize,
        captures: Vec<String>,
        maximum: u64,
    },
}

#[derive(Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(untagged)]
pub(super) enum Scalar {
    Text(String),
    Integer(u64),
    Boolean(bool),
}

impl Scalar {
    fn belongs_to(&self, kind: Kind) -> bool {
        match (self, kind) {
            (Self::Text(_), Kind::Text | Kind::StaticText) | (Self::Boolean(_), Kind::Boolean) => {
                true
            }
            (Self::Integer(n), Kind::Decimal(bits)) => {
                *n <= unsigned_maximum(bits).expect("validated capture width")
            }
            _ => false,
        }
    }

    fn rust_pattern(&self) -> String {
        match self {
            Self::Text(v) => format!("{v:?}"),
            Self::Integer(v) => v.to_string(),
            Self::Boolean(v) => v.to_string(),
        }
    }
}

impl Constraint {
    pub(super) fn row(&self) -> usize {
        match self {
            Self::AllowedTuples { after_row, .. } | Self::CheckedSum { after_row, .. } => {
                *after_row
            }
        }
    }

    fn input_ids(&self) -> &[String] {
        match self {
            Self::AllowedTuples { captures, .. } | Self::CheckedSum { captures, .. } => captures,
        }
    }

    pub(super) fn validate(&self, captures: &Captures<'_>) -> Result<()> {
        let inputs = self.input_ids();
        if inputs.is_empty() || inputs.iter().collect::<BTreeSet<_>>().len() != inputs.len() {
            return Err("constraint requires nonempty distinct capture identities".into());
        }
        let kinds = inputs
            .iter()
            .map(|input| {
                captures
                    .get(input.as_str())
                    .copied()
                    .ok_or_else(|| format!("constraint references an unavailable capture: {input}"))
            })
            .collect::<Result<Vec<_>>>()?;
        match self {
            Self::AllowedTuples { allowed, .. } => {
                if allowed.is_empty()
                    || allowed.iter().collect::<BTreeSet<_>>().len() != allowed.len()
                    || allowed.iter().any(|row| {
                        row.len() != kinds.len()
                            || row
                                .iter()
                                .zip(&kinds)
                                .any(|(value, kind)| !value.belongs_to(*kind))
                    })
                {
                    return Err(
                        "tuple constraint requires distinct complete rows of matching scalar types"
                            .into(),
                    );
                }
            }
            Self::CheckedSum { .. } => {
                if kinds.iter().any(|kind| !matches!(kind, Kind::Decimal(_))) {
                    return Err("checked sum requires unsigned captures".into());
                }
            }
        }
        Ok(())
    }

    pub(super) fn rust_condition(&self, captures: &BTreeMap<&str, usize>) -> String {
        let values = self
            .input_ids()
            .iter()
            .map(|id| format!("__capture_{}", captures[id.as_str()]));
        match self {
            Self::AllowedTuples { allowed, .. } => {
                let subjects = values.collect::<Vec<_>>().join(", ");
                let patterns = allowed
                    .iter()
                    .map(|row| {
                        let elements = row
                            .iter()
                            .map(Scalar::rust_pattern)
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("({elements},)")
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("::core::matches!(({subjects},), {patterns})")
            }
            Self::CheckedSum { maximum, .. } => {
                let terms = values
                    .map(|value| format!("::core::primitive::u64::from({value})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "[{terms}].into_iter().try_fold(0_u64, ::core::primitive::u64::checked_add)? <= {maximum}"
                )
            }
        }
    }
}
