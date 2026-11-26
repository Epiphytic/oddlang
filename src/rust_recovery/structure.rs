use super::{Result, cargo_name, hex};
use oxigraph::model::{Literal, NamedNode, Term, Triple};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

const NS: &str = "urn:oddlang:rust-structure:v1:";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Inventory {
    pub crate_name: String,
    pub compiler: Option<String>,
    pub declarations: Vec<Declaration>,
    pub edges: Vec<Edge>,
    pub gaps: Vec<Gap>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Declaration {
    id: String,
    name: Option<String>,
    kind: String,
    owner: Option<String>,
    visibility: String,
    macro_generated: bool,
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    signature: Option<Signature>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    abi: String,
    safety: String,
    variadic: bool,
    inputs: Vec<TypeShape>,
    output: TypeShape,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum TypeShape {
    Primitive {
        name: String,
    },
    Definition {
        id: String,
        arguments: Vec<TypeShape>,
    },
    Parameter {
        index: u32,
    },
    Tuple {
        elements: Vec<TypeShape>,
    },
    Reference {
        mutable: bool,
        target: Box<TypeShape>,
    },
    Pointer {
        mutable: bool,
        target: Box<TypeShape>,
    },
    Slice {
        element: Box<TypeShape>,
    },
    Unknown,
}

impl TypeShape {
    fn valid(&self, known: &impl Fn(&str) -> bool) -> bool {
        match self {
            Self::Primitive { name } => PRIMITIVES.contains(&name.as_str()),
            Self::Definition { id, arguments } => {
                known(id) && arguments.iter().all(|ty| ty.valid(known))
            }
            Self::Tuple { elements } => elements.iter().all(|ty| ty.valid(known)),
            Self::Reference { target, .. }
            | Self::Pointer { target, .. }
            | Self::Slice { element: target } => target.valid(known),
            Self::Parameter { .. } | Self::Unknown => true,
        }
    }

    fn emit(
        &self,
        subject: &str,
        node: &impl Fn(&str) -> String,
        add: &mut impl FnMut(&str, &str, Term),
    ) {
        let (kind, children): (_, &[_]) = match self {
            Self::Primitive { name } => {
                add(subject, "primitive", Literal::from(name.as_str()).into());
                ("primitive", &[])
            }
            Self::Definition { id, arguments } => {
                let definition = node(id);
                add(
                    subject,
                    "definition",
                    NamedNode::new_unchecked(&definition).into(),
                );
                if id.starts_with("external::") {
                    add(
                        &definition,
                        "externalPath",
                        Literal::from(id.as_str()).into(),
                    );
                }
                ("definition", arguments)
            }
            Self::Parameter { index } => {
                add(subject, "parameterIndex", Literal::from(*index).into());
                ("parameter", &[])
            }
            Self::Tuple { elements } => ("tuple", elements),
            Self::Reference { mutable, target } | Self::Pointer { mutable, target } => {
                add(subject, "mutable", Literal::from(*mutable).into());
                let kind = if matches!(self, Self::Reference { .. }) {
                    "reference"
                } else {
                    "pointer"
                };
                (kind, std::slice::from_ref(target.as_ref()))
            }
            Self::Slice { element } => ("slice", std::slice::from_ref(element.as_ref())),
            Self::Unknown => ("unknown", &[]),
        };
        add(subject, "typeKind", Literal::from(kind).into());
        for (position, ty) in children.iter().enumerate() {
            let child = format!("{subject}:type:{position}");
            add(
                subject,
                "typeChild",
                NamedNode::new_unchecked(&child).into(),
            );
            add(&child, "position", Literal::from(position as u64).into());
            ty.emit(&child, node, add);
        }
    }
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(super) struct Edge {
    from: String,
    relation: Relation,
    to: String,
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum Relation {
    Implements,
    SignatureType,
    Reexport,
    Call,
    Read,
    Write,
    Dispatch,
    MutableBorrow,
    SharedBorrow,
    StaticReference,
    VisibilityScope,
    GenericBound,
    FunctionReference,
}

impl Relation {
    fn valid_endpoints(&self, source: &Declaration, target: Option<&Declaration>) -> bool {
        let target_kind = target.map(|decl| decl.kind.as_str());
        let body = matches!(
            source.kind.as_str(),
            "function"
                | "associated-function"
                | "closure"
                | "constant"
                | "associated-constant"
                | "static"
        );
        match self {
            Self::Implements => {
                source.kind == "implementation"
                    && target_kind.is_none_or(|kind| matches!(kind, "trait" | "trait-alias"))
            }
            Self::Reexport => matches!(source.kind.as_str(), "extern-crate" | "import"),
            Self::Call | Self::FunctionReference | Self::Dispatch => {
                body && target_kind.is_none_or(|kind| {
                    matches!(
                        kind,
                        "function" | "associated-function" | "constructor" | "closure"
                    )
                })
            }
            Self::Read | Self::Write | Self::MutableBorrow | Self::SharedBorrow => {
                body && target_kind.is_none_or(|kind| {
                    matches!(
                        kind,
                        "constant" | "associated-constant" | "field" | "static"
                    )
                })
            }
            Self::StaticReference => body && target_kind.is_none_or(|kind| kind == "static"),
            Self::VisibilityScope => {
                source.visibility == "restricted" && matches!(target_kind, Some("crate" | "module"))
            }
            Self::SignatureType | Self::GenericBound => true,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Implements => "implements",
            Self::SignatureType => "signature-type",
            Self::Reexport => "reexport",
            Self::Call => "call",
            Self::Read => "read",
            Self::Write => "write",
            Self::Dispatch => "dispatch",
            Self::MutableBorrow => "mutable-borrow",
            Self::SharedBorrow => "shared-borrow",
            Self::StaticReference => "static-reference",
            Self::VisibilityScope => "visibility-scope",
            Self::GenericBound => "generic-bound",
            Self::FunctionReference => "function-reference",
        }
    }
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(super) struct Gap {
    owner: String,
    kind: String,
}

impl Inventory {
    pub(super) fn load(bytes: &[u8]) -> Result<Self> {
        let value: Self =
            serde_json::from_slice(bytes).map_err(|err| format!("compiler inventory: {err}"))?;
        if !cargo_name(&value.crate_name)
            || value.compiler.as_deref().is_some_and(|id| !hex(id, 40))
        {
            return Err("invalid compiler inventory crate or commit".into());
        }
        let mut ids = BTreeMap::new();
        let root_id = format!("local::{}", value.crate_name);
        let mut roots = 0;
        for decl in &value.declarations {
            if !decl.id.starts_with("local::")
                || !valid_id(&decl.id)
                || ids.insert(decl.id.as_str(), decl).is_some()
            {
                return Err("invalid or duplicate declaration ID".into());
            }
            if !KINDS.contains(&decl.kind.as_str())
                || !matches!(
                    decl.visibility.as_str(),
                    "public" | "restricted" | "private" | "crate" | "inherited"
                )
                || decl.name.as_deref().is_some_and(|name| !identifier(name))
                || decl
                    .file
                    .as_deref()
                    .is_some_and(|path| path.len() > 1024 || path.chars().any(char::is_control))
                || decl.line == Some(0)
                || decl.line.is_some() != decl.column.is_some()
                || decl.file.is_some() != decl.line.is_some()
            {
                return Err(format!("invalid descriptor for {}", decl.id));
            }
            roots +=
                usize::from(decl.owner.is_none() && decl.kind == "crate" && decl.id == root_id);
        }
        if roots != 1 {
            return Err("expected exactly one crate declaration".into());
        }
        let known = |target: &str| {
            ids.contains_key(target) || (target.starts_with("external::") && valid_id(target))
        };
        for decl in &value.declarations {
            if let Some(signature) = &decl.signature {
                if !matches!(decl.kind.as_str(), "function" | "associated-function")
                    || !identifier(&signature.abi)
                    || !matches!(signature.safety.as_str(), "safe" | "unsafe")
                    || !signature.output.valid(&known)
                    || !signature.inputs.iter().all(|ty| ty.valid(&known))
                {
                    return Err(format!("invalid signature for {}", decl.id));
                }
            }
            match &decl.owner {
                None if decl.kind == "crate" => (),
                Some(owner)
                    if decl.kind != "crate"
                        && ids.contains_key(owner.as_str())
                        && decl
                            .id
                            .strip_prefix(owner)
                            .is_some_and(|suffix| suffix.starts_with("::")) =>
                {
                    ()
                }
                _ => return Err(format!("invalid declaration owner for {}", decl.id)),
            }
        }
        let mut edges = BTreeSet::new();
        for edge in &value.edges {
            let source = ids.get(edge.from.as_str());
            let target = ids.get(edge.to.as_str());
            if !known(&edge.to)
                || !source
                    .is_some_and(|source| edge.relation.valid_endpoints(source, target.copied()))
                || !edges.insert(edge)
            {
                return Err(format!(
                    "invalid or duplicate {} relationship",
                    edge.relation.label()
                ));
            }
        }
        let mut gaps = BTreeSet::new();
        for gap in &value.gaps {
            if !ids.contains_key(gap.owner.as_str())
                || !GAPS.contains(&gap.kind.as_str())
                || !gaps.insert(gap)
            {
                return Err("invalid or duplicate compiler inventory gap".into());
            }
        }
        Ok(value)
    }

    pub(super) fn graph(&self, package: &str) -> Vec<u8> {
        let scope = format!("{NS}{}:{}", escape(package), escape(&self.crate_name));
        let id = |name: &str| format!("{scope}:{}", escape(name));
        let mut triples = BTreeSet::new();
        let mut add = |subject: &str, predicate: &str, target: Term| {
            triples.insert(
                Triple::new(
                    NamedNode::new_unchecked(subject),
                    NamedNode::new_unchecked(format!("{NS}{predicate}")),
                    target,
                )
                .to_string(),
            );
        };
        for decl in &self.declarations {
            let iri = id(&decl.id);
            add(&iri, "kind", Literal::from(decl.kind.as_str()).into());
            add(
                &iri,
                "visibility",
                Literal::from(decl.visibility.as_str()).into(),
            );
            add(
                &iri,
                "macroGenerated",
                Literal::from(decl.macro_generated).into(),
            );
            if let Some(name) = &decl.name {
                add(&iri, "name", Literal::from(name.as_str()).into());
            }
            if let Some(owner) = &decl.owner {
                add(&iri, "owner", NamedNode::new_unchecked(id(owner)).into());
            }
            if let Some(signature) = &decl.signature {
                add(&iri, "abi", Literal::from(signature.abi.as_str()).into());
                add(
                    &iri,
                    "safety",
                    Literal::from(signature.safety.as_str()).into(),
                );
                add(&iri, "variadic", Literal::from(signature.variadic).into());
                let output = format!("{iri}:output");
                add(&iri, "output", NamedNode::new_unchecked(&output).into());
                signature.output.emit(&output, &id, &mut add);
                for (position, ty) in signature.inputs.iter().enumerate() {
                    let input = format!("{iri}:input:{position}");
                    add(&iri, "input", NamedNode::new_unchecked(&input).into());
                    add(&input, "position", Literal::from(position as u64).into());
                    ty.emit(&input, &id, &mut add);
                }
            }
        }
        for edge in &self.edges {
            let target = id(&edge.to);
            add(
                &id(&edge.from),
                edge.relation.label(),
                NamedNode::new_unchecked(&target).into(),
            );
            if edge.to.starts_with("external::") {
                add(
                    &target,
                    "externalPath",
                    Literal::from(edge.to.as_str()).into(),
                );
            }
        }
        for gap in &self.gaps {
            add(
                &id(&gap.owner),
                "gap",
                Literal::from(gap.kind.as_str()).into(),
            );
        }
        let mut bytes = Vec::new();
        for triple in triples {
            bytes.extend_from_slice(triple.as_bytes());
            bytes.extend_from_slice(b" .\n");
        }
        bytes
    }
}

fn escape(name: &str) -> String {
    let mut output = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_".contains(&byte) {
            output.push(char::from(byte));
        } else {
            write!(output, "%{byte:02X}").expect("writing to a string");
        }
    }
    output
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|ch| ch.is_alphanumeric() || "_-'".contains(ch))
}

fn valid_id(value: &str) -> bool {
    value.len() <= 2048
        && value.split("::").all(|segment| {
            let segment = match segment.rsplit_once('#') {
                Some((base, index))
                    if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    base
                }
                Some(_) => return false,
                None => segment,
            };
            identifier(segment)
                || segment
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .is_some_and(valid_namespace)
        })
}

fn valid_namespace(segment: &str) -> bool {
    matches!(
        segment,
        "type"
            | "macro"
            | "lifetime"
            | "impl"
            | "use"
            | "extern"
            | "asm"
            | "closure"
            | "ctor"
            | "constant"
            | "opaque"
            | "opaque-lifetime"
            | "associated-type"
            | "coroutine"
            | "nested-static"
            | "anonymous-name"
    ) || segment.strip_prefix("crate-").is_some_and(|id| hex(id, 16))
}

const PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "!", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32",
    "u64", "u128", "usize", "f16", "f32", "f64", "f128",
];
const KINDS: &[&str] = &[
    "crate",
    "module",
    "struct",
    "union",
    "enum",
    "variant",
    "field",
    "trait",
    "trait-alias",
    "type-alias",
    "function",
    "constant",
    "static",
    "implementation",
    "import",
    "extern-crate",
    "foreign-module",
    "foreign-type",
    "macro",
    "constructor",
    "associated-type",
    "associated-constant",
    "associated-function",
    "type-parameter",
    "const-parameter",
    "lifetime-parameter",
    "opaque-type",
    "closure",
    "global-assembly",
];
const GAPS: &[&str] = &[
    "dynamic-dispatch",
    "compiler-shim-dispatch",
    "generic-dispatch",
    "indirect-call",
    "signature-lifetimes",
    "signature-type-shape",
    "signature-const-arguments",
    "reexport-alias",
    "static-access-classification",
    "body-mir-optimization",
    "generic-bounds",
    "declaration-order",
    "package-instance-identities",
    "unresolved-identity",
    "body-coverage",
    "typed-declaration-shape",
    "implicit-drop",
    "skipped-body",
    "nested-field-projection",
    "ordinal-identity",
];
