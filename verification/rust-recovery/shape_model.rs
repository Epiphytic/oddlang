//! Data shape graph and typed compiler observation envelope.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const VOCABULARY: &str = "oddlang-data-shapes-v2";
pub const PREVIOUS_VOCABULARY: &str = "oddlang-data-shapes-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub compiler: String,
    #[serde(rename = "crateName")]
    pub crate_name: String,
    pub graph: Graph,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub vocabulary: String,
    pub declarations: Vec<Declaration>,
    pub gaps: BTreeSet<Gap>,
}

#[derive(Deserialize, Serialize)]
pub struct Declaration {
    pub id: String,
    pub name: String,
    pub parameters: Vec<Parameter>,
    #[serde(flatten)]
    pub shape: DeclarationShape,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum DeclarationShape {
    Product { form: Form, fields: Vec<Field> },
    Sum { variants: Vec<Variant> },
    Union { fields: Vec<Field> },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    Record,
    Tuple,
    Unit,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    pub position: u32,
    pub kind: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub position: usize,
    pub form: Form,
    pub fields: Vec<Field>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub owner: String,
    pub name: Option<String>,
    pub position: usize,
    #[serde(rename = "type")]
    pub shape: Shape,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    pub parameter: String,
    pub name: String,
    pub position: u32,
    pub value: Shape,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TupleElement {
    pub position: usize,
    #[serde(rename = "type")]
    pub shape: Shape,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum Shape {
    Boolean,
    UnicodeScalar,
    Text,
    Never,
    SignedInteger {
        bits: u64,
    },
    UnsignedInteger {
        bits: u64,
    },
    TargetInteger {
        bits: u64,
        signed: bool,
    },
    FloatingPoint {
        bits: u64,
    },
    Named {
        id: String,
        arguments: Vec<Argument>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ownership: Option<String>,
    },
    Parameter {
        id: String,
    },
    StaticLifetime,
    Tuple {
        elements: Vec<TupleElement>,
    },
    Reference {
        ownership: String,
        lifetime: Box<Shape>,
        target: Box<Shape>,
    },
    RawPointer {
        mutable: bool,
        target: Box<Shape>,
    },
    Array {
        element: Box<Shape>,
        extent: ArrayExtent,
    },
    Slice {
        element: Box<Shape>,
    },
    Unresolved {
        reason: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum ArrayExtent {
    Fixed { count: u64 },
    Parameter { id: String },
    Unresolved,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap {
    pub owner: String,
    pub reason: String,
}
