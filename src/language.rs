//! Closed, typed in-memory representation of ledger operations.
use crate::model::{CORE, Field, allowed, core, instances, iri, literal, named, node, one};
use oxigraph::model::{
    vocab::{rdf, xsd},
    *,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

type Result<T> = std::result::Result<T, String>;
pub const SOURCE: &str = "urn:oddlang:ledger:source";
pub const DESTINATION: &str = "urn:oddlang:ledger:destination";
pub const ASSIGNMENT_CONFLICT: &str = "urn:oddlang:ledger:error:AssignmentConflict";
pub const FIELD_OUT_OF_RANGE: &str = "urn:oddlang:ledger:error:FieldOutOfRange";
pub const AMOUNT: &str = "urn:oddlang:ledger:amount";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    NotEqual,
    Greater,
    GreaterEqual,
    LessEqual,
    Add,
    Subtract,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Argument(String),
    Exists(String),
    Read { account: String, field: String },
    Constant(i128),
    Binary(Binary, Box<Expr>, Box<Expr>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guard {
    pub iri: String,
    pub priority: u64,
    pub condition: Expr,
    pub error: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub account: String,
    pub field: String,
    pub value: Expr,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub guards: Vec<Guard>,
    pub assignments: Vec<Assignment>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Type {
    Bool,
    Text,
    Int(i128, i128),
}

fn class(g: &Graph, s: &NamedOrBlankNode, at: &str) -> Result<String> {
    let mut it = g.objects_for_subject_predicate(s, rdf::TYPE);
    match (it.next(), it.next()) {
        (Some(t), None) => iri(t, at),
        _ => Err(format!("{at}: expected exactly one declaration type")),
    }
}
fn check_class(g: &Graph, s: &NamedOrBlankNode, kind: &str, at: &str) -> Result<()> {
    if class(g, s, at)? != core(kind).as_str() {
        return Err(format!("{at}: expected {kind}"));
    }
    Ok(())
}
fn reference(g: &Graph, s: &NamedOrBlankNode, p: &str, at: &str) -> Result<String> {
    iri(one(g, s, p, at)?, &format!("{at}/{p}"))
}
fn integer<T: std::str::FromStr>(g: &Graph, s: &NamedOrBlankNode, p: &str, at: &str) -> Result<T> {
    let path = format!("{at}/{p}");
    literal(one(g, s, p, at)?, xsd::INTEGER, &path)?
        .parse()
        .map_err(|_| format!("{path}: invalid integer or outside supported range"))
}

pub(crate) fn behavior_class(iri: &str) -> bool {
    matches!(
        iri.strip_prefix(CORE),
        Some(
            "Argument"
                | "Guard"
                | "Error"
                | "Assign"
                | "Exists"
                | "Read"
                | "Constant"
                | "NotEqual"
                | "Greater"
                | "GreaterEqual"
                | "LessEqual"
                | "Add"
                | "Subtract"
        )
    )
}

const MAX_DEPTH: usize = 24;
const MAX_EXPANDED_NODES: usize = 256;

struct Scope<'a> {
    owned: &'a mut HashSet<NamedOrBlankNode>,
    graph: &'a Graph,
    expanded: usize,
    fields: &'a BTreeMap<String, Field>,
    exists: BTreeSet<String>,
}

impl Scope<'_> {
    fn account(&self, s: &NamedOrBlankNode, at: &str, require_exists: bool) -> Result<String> {
        let id = reference(self.graph, s, "account", at)?;
        if !matches!(id.as_str(), SOURCE | DESTINATION) {
            return Err(format!("{at}/account: expected account ID argument"));
        }
        if require_exists && !self.exists.contains(&id) {
            return Err(format!(
                "{at}/account: missing earlier existence guard for {id}"
            ));
        }
        Ok(id)
    }
    fn field(&self, s: &NamedOrBlankNode, at: &str) -> Result<(String, Type)> {
        let id = reference(self.graph, s, "field", at)?;
        let f = self
            .fields
            .get(&id)
            .ok_or_else(|| format!("{at}/field: unknown field {id}"))?;
        let ty = f
            .range
            .map_or(Type::Text, |(lo, hi)| Type::Int(lo.into(), hi.into()));
        Ok((id, ty))
    }
    fn expr(
        &mut self,
        term: TermRef<'_>,
        path: &str,
        stack: &mut HashSet<NamedOrBlankNode>,
    ) -> Result<(Expr, Type)> {
        if stack.len() == MAX_DEPTH {
            return Err(format!("{path}: expression exceeds depth {MAX_DEPTH}"));
        }
        if self.expanded == MAX_EXPANDED_NODES {
            return Err(format!(
                "{path}: operation exceeds {MAX_EXPANDED_NODES} expanded expression nodes"
            ));
        }
        self.expanded += 1;
        let s = node(term, path)?;
        self.owned.insert(s.clone());
        let at = match &s {
            NamedOrBlankNode::NamedNode(n) => n.as_str(),
            _ => path,
        };
        if !stack.insert(s.clone()) {
            return Err(format!("{at}: cyclic expression"));
        }
        let g = self.graph;
        let class = class(g, &s, at)?;
        let lowered = match class.strip_prefix(CORE) {
            Some("Argument") => {
                let id = iri(term, path)?;
                let ty = match id.as_str() {
                    SOURCE | DESTINATION => Type::Text,
                    AMOUNT => Type::Int(i64::MIN.into(), i64::MAX.into()),
                    _ => return Err(format!("{at}: unbound argument")),
                };
                (Expr::Argument(id), ty)
            }
            Some("Constant") => {
                allowed(g, &s, &["integer"], at)?;
                let n = integer(g, &s, "integer", at)?;
                (Expr::Constant(n), Type::Int(n, n))
            }
            Some("Read") => {
                allowed(g, &s, &["account", "field"], at)?;
                let account = self.account(&s, at, true)?;
                let (field, ty) = self.field(&s, at)?;
                (Expr::Read { account, field }, ty)
            }
            Some("Exists") => {
                allowed(g, &s, &["account"], at)?;
                (Expr::Exists(self.account(&s, at, false)?), Type::Bool)
            }
            Some("NotEqual" | "Greater" | "GreaterEqual" | "LessEqual" | "Add" | "Subtract") => {
                allowed(g, &s, &["left", "right"], at)?;
                let (left, lt) =
                    self.expr(one(g, &s, "left", at)?, &format!("{at}/left"), stack)?;
                let (right, rt) =
                    self.expr(one(g, &s, "right", at)?, &format!("{at}/right"), stack)?;
                let (op, ty) = binary(&class, lt, rt, at)?;
                (Expr::Binary(op, Box::new(left), Box::new(right)), ty)
            }
            _ => return Err(format!("{at}: unsupported expression {class}")),
        };
        stack.remove(&s);
        Ok(lowered)
    }
}

fn binary(class: &str, left: Type, right: Type, at: &str) -> Result<(Binary, Type)> {
    if class == core("NotEqual").as_str() && left == Type::Text && right == Type::Text {
        return Ok((Binary::NotEqual, Type::Bool));
    }
    let (Type::Int(a, b), Type::Int(c, d)) = (left, right) else {
        return Err(format!("{at}: invalid operand type"));
    };
    let (op, lo, hi) = match class.strip_prefix(CORE) {
        Some("Greater") => return Ok((Binary::Greater, Type::Bool)),
        Some("GreaterEqual") => return Ok((Binary::GreaterEqual, Type::Bool)),
        Some("LessEqual") => return Ok((Binary::LessEqual, Type::Bool)),
        Some("Add") => (Binary::Add, a.checked_add(c), b.checked_add(d)),
        Some("Subtract") => (Binary::Subtract, a.checked_sub(d), b.checked_sub(c)),
        _ => return Err(format!("{at}: invalid operand type")),
    };
    match (lo, hi) {
        (Some(lo), Some(hi)) => Ok((op, Type::Int(lo, hi))),
        _ => Err(format!("{at}: interval exceeds i128")),
    }
}

fn named_set(g: &Graph, s: &NamedOrBlankNode, p: &str, at: &str) -> Result<BTreeSet<String>> {
    g.objects_for_subject_predicate(s, &core(p))
        .map(|t| iri(t, &format!("{at}/{p}")))
        .collect()
}

pub(crate) fn lower(
    g: &Graph,
    fields: &BTreeMap<String, Field>,
    owned: &mut HashSet<NamedOrBlankNode>,
) -> Result<BTreeMap<String, Operation>> {
    let mut operations = BTreeMap::new();
    for op in instances(g, "Operation")? {
        let s = named(&op);
        let args = named_set(g, &s, "argument", &op)?;
        if args != [SOURCE, DESTINATION, AMOUNT].map(str::to_owned).into() {
            return Err(format!(
                "{op}: expected source, destination and amount arguments"
            ));
        }
        for id in args {
            let arg = named(&id);
            owned.insert(arg.clone());
            check_class(g, &arg, "Argument", &id)?;
            allowed(g, &arg, &["type"], &id)?;
            let ty = if id == AMOUNT { "I64" } else { "Text" };
            if reference(g, &arg, "type", &id)? != core(ty).as_str() {
                return Err(format!("{id}: expected {ty}"));
            }
        }
        let mut order = BTreeMap::new();
        for id in named_set(g, &s, "guard", &op)? {
            let guard = named(&id);
            owned.insert(guard.clone());
            check_class(g, &guard, "Guard", &id)?;
            allowed(g, &guard, &["priority", "condition", "error"], &id)?;
            let priority: u64 = integer(g, &guard, "priority", &id)?;
            if priority == 0 {
                return Err(format!("{id}: expected positive priority"));
            }
            if order.insert(priority, id).is_some() {
                return Err(format!("{op}: duplicate priority {priority}"));
            }
        }
        if order.is_empty() {
            return Err(format!("{op}: expected at least one guard"));
        }
        let mut scope = Scope {
            owned,
            graph: g,
            expanded: 0,
            fields,
            exists: BTreeSet::new(),
        };
        let mut guards = Vec::new();
        for (priority, id) in order {
            let guard = named(&id);
            let error = reference(g, &guard, "error", &id)?;
            if error
                .strip_prefix("urn:oddlang:ledger:error:")
                .is_none_or(str::is_empty)
            {
                return Err(format!("{id}/error: invalid guard error IRI {error}"));
            }
            if matches!(error.as_str(), ASSIGNMENT_CONFLICT | FIELD_OUT_OF_RANGE) {
                return Err(format!("{id}/error: reserved commit error {error}"));
            }
            scope.owned.insert(named(&error));
            check_class(g, &named(&error), "Error", &error)?;
            allowed(g, &named(&error), &[], &error)?;
            let (condition, ty) = scope.expr(
                one(g, &guard, "condition", &id)?,
                &format!("{id}/condition"),
                &mut HashSet::new(),
            )?;
            if ty != Type::Bool {
                return Err(format!("{id}/condition: expected boolean"));
            }
            if let Expr::Exists(account) = &condition {
                scope.exists.insert(account.clone());
            }
            guards.push(Guard {
                iri: id,
                priority,
                error,
                condition,
            });
        }
        // Resolve targets first so anonymous assignments are lowered in stable
        // target order, including when an operation reaches its expansion limit.
        let mut targets = BTreeMap::new();
        let mut errors = BTreeSet::new();
        for t in g.objects_for_subject_predicate(&s, &core("assign")) {
            let path = format!("{op}/assign");
            let mut parse = || -> Result<_> {
                let s = node(t, &path)?;
                scope.owned.insert(s.clone());
                let at = match &s {
                    NamedOrBlankNode::NamedNode(n) => n.as_str(),
                    _ => &path,
                };
                check_class(g, &s, "Assign", at)?;
                allowed(g, &s, &["account", "field", "value"], at)?;
                let account = scope.account(&s, at, true)?;
                let (field, ty) = scope.field(&s, at)?;
                if field == crate::model::ID {
                    return Err(format!("{at}: cannot assign account key"));
                }
                let term = one(g, &s, "value", at)?;
                let path = format!("{at}/{account}/{field}/value");
                Ok(((account, field), (term, ty, path)))
            };
            match parse() {
                Ok((key, rhs)) => {
                    if targets.insert(key.clone(), rhs).is_some() {
                        errors.insert(format!(
                            "{op}: duplicate assignment target {} {}",
                            key.0, key.1
                        ));
                    }
                }
                Err(e) => {
                    errors.insert(e);
                }
            }
        }
        if let Some(e) = errors.pop_first() {
            return Err(e);
        }
        if targets.is_empty() {
            return Err(format!("{op}: expected at least one assignment"));
        }
        let mut assignments = Vec::new();
        for ((account, field), (term, ty, path)) in targets {
            let (value, vt) = scope.expr(term, &path, &mut HashSet::new())?;
            if std::mem::discriminant(&vt) != std::mem::discriminant(&ty) {
                return Err(format!("{path}: assignment type mismatch"));
            }
            assignments.push(Assignment {
                account,
                field,
                value,
            });
        }
        operations.insert(
            op,
            Operation {
                guards,
                assignments,
            },
        );
    }
    Ok(operations)
}
