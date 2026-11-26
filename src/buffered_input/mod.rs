//! Portable bounded lookahead storage with independently selected error and erasure policies.
mod recovery;
mod rust;
use crate::wire_schema::iri;
pub use recovery::recover;
pub(crate) use rust::build_bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, String>;
const FORMAT: &str = "oddlang-buffered-input-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Role {
    Input,
    Storage,
    Begin,
    End,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    capacity: u64,
    field_order: Vec<Role>,
    transitions: Transitions,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Transitions {
    empty_read: EmptyRead,
    refill_error: RefillError,
    consumed_bytes: ConsumedBytes,
    release_storage: Release,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum EmptyRead {
    RefillIfExhausted,
    ReturnZero,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RefillError {
    ResetBeginRetainEnd,
    DiscardWindow,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ConsumedBytes {
    Erase,
    Retain,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Release {
    Erase,
}

impl Graph {
    fn validate(&self) -> Result<()> {
        iri(&self.id)?;
        if self.format != FORMAT || self.capacity == 0 {
            return Err("expected a nonempty fixed buffer".into());
        }
        if self.capacity > max_capacity(64) {
            return Err("buffered-input capacity exceeds the native layout bound".into());
        }
        if self.field_order.len() != 4
            || self.field_order.iter().collect::<BTreeSet<_>>().len() != 4
        {
            return Err("each buffered-input field role must occur exactly once".into());
        }
        Ok(())
    }
}

// A buffer must fit alongside at least its two pointer-sized cursors.
fn max_capacity(bits: u64) -> u64 {
    (u64::MAX >> (65 - bits)) - 2 * (bits / 8)
}

/// Names used unqualified in the generated Rust leaf.
fn reserved(name: &str) -> bool {
    matches!(name, "u8" | "usize" | "Ok")
}
