//! Role-neutral semantic declarations in deterministic IRI order.
use super::{H, HttpError};
use crate::contracts::Pin;
use oxigraph::model::{
    NamedNode, Term,
    vocab::{rdf, xsd},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Iri(String);
impl Iri {
    pub fn new(value: impl Into<String>) -> Result<Self, HttpError> {
        NamedNode::new(value.into())
            .map(|n| Self(n.into_string()))
            .map_err(|e| e.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for Iri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::borrow::Borrow<str> for Iri {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpContract {
    pub entry: String,
    pub source_files: BTreeMap<String, Vec<u8>>,
    pub scenario_files: BTreeSet<String>,
    pub identity: Iri,
    pub version: String,
    pub rule_set_version: u32,
    pub dependencies: Vec<Pin>,
    pub operations: BTreeMap<Iri, Operation>,
    pub records: BTreeMap<Iri, Record>,
    pub fields: BTreeMap<Iri, Field>,
    pub errors: BTreeMap<Iri, u16>,
    pub observations: BTreeMap<Iri, Observation>,
    pub constraints: BTreeMap<Iri, Constraint>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub method: String,
    pub path: String,
    pub request: Iri,
    pub response: Iri,
    pub status: u16,
    pub roles: BTreeSet<Iri>,
    pub errors: BTreeSet<Iri>,
    pub observations: BTreeSet<Iri>,
    pub behavior: Option<Iri>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub fields: BTreeSet<Iri>,
    pub constraints: BTreeSet<Iri>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    pub native_name: String,
    pub wire_name: String,
    pub shape: Iri,
    pub constraints: BTreeSet<Iri>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Constraint {
    Required,
    Closed,
    Bounds { min: i64, max: i64 },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observation {
    Lookup,
    Transfer,
}

pub(super) type Properties = BTreeMap<String, std::collections::HashSet<Term>>;
pub(super) type Nodes = BTreeMap<Iri, Properties>;
struct Decl<'a> {
    id: &'a Iri,
    props: &'a Properties,
}
impl Decl<'_> {
    fn error(&self, key: &str, why: &str) -> String {
        format!("{} {H}{key}: {why}", self.id)
    }
    fn values(&self, k: &str) -> impl Iterator<Item = &Term> {
        self.props.get(&format!("{H}{k}")).into_iter().flatten()
    }
    fn one(&self, k: &str) -> Result<&Term, String> {
        let mut v = self.values(k);
        match (v.next(), v.next()) {
            (Some(t), None) => Ok(t),
            _ => Err(self.error(k, "expected exactly one value")),
        }
    }
    fn literal(&self, k: &str, ty: &str) -> Result<&str, String> {
        match self.one(k)? {
            Term::Literal(l) if l.datatype().as_str() == ty => Ok(l.value()),
            _ => Err(self.error(k, "invalid literal type")),
        }
    }
    fn string(&self, k: &str) -> Result<String, String> {
        Ok(self.literal(k, xsd::STRING.as_str())?.to_owned())
    }
    fn integer(&self, k: &str) -> Result<i64, String> {
        self.literal(k, xsd::INTEGER.as_str())?
            .parse()
            .map_err(|_| self.error(k, "expected i64"))
    }
    fn iri(&self, k: &str) -> Result<Iri, String> {
        match self.one(k)? {
            Term::NamedNode(n) => Iri::new(n.as_str()),
            _ => Err(self.error(k, "expected IRI")),
        }
    }
    fn refs(&self, k: &str) -> Result<BTreeSet<Iri>, String> {
        self.values(k)
            .map(|t| match t {
                Term::NamedNode(n) => Iri::new(n.as_str()),
                _ => Err(self.error(k, "expected IRI")),
            })
            .collect()
    }
    fn keys(&self, keys: &[&str]) -> Result<(), String> {
        for key in self.props.keys() {
            if key != rdf::TYPE.as_str() && !keys.iter().any(|k| key == &format!("{H}{k}")) {
                return Err(format!("{} {key}: unknown predicate", self.id));
            }
        }
        Ok(())
    }
    fn status(&self) -> Result<u16, String> {
        u16::try_from(self.integer("status")?)
            .map_err(|_| self.error("status", "invalid HTTP status"))
    }
}

pub(super) fn parse(
    nodes: Nodes,
    dependencies: Vec<Pin>,
    ledger_symbols: &BTreeMap<Iri, crate::language::Operation>,
) -> Result<HttpContract, String> {
    let mut header = None;
    let (mut operations, mut records, mut fields, mut constraints, mut errors, mut observations) = (
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    for (id, props) in &nodes {
        let d = Decl { id, props };
        let types = props.get(rdf::TYPE.as_str());
        let kind = types
            .filter(|ts| ts.len() == 1)
            .and_then(|ts| ts.iter().next())
            .and_then(|t| match t {
                Term::NamedNode(n) => n.as_str().strip_prefix(H),
                _ => None,
            })
            .ok_or_else(|| format!("{id} rdf:type: expected one HTTP declaration type"))?;
        match kind {
            "Contract" => {
                d.keys(&[
                    "identity",
                    "version",
                    "ruleSetVersion",
                    "localImport",
                    "dependency",
                    "scenarioFile",
                ])?;
                let identity = d.iri("identity")?;
                let version = d.string("version")?;
                if !super::version_ok(&version) || d.integer("ruleSetVersion")? != 1 {
                    return Err(format!("{id}: unsupported version or rule set"));
                }
                let expected = dependencies
                    .iter()
                    .map(|p| Iri::new(format!("{}:{}", p.identity, p.version)))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                if expected != d.refs("dependency")? {
                    return Err(d.error("dependency", "source and pins disagree"));
                }
                if header.replace((identity, version)).is_some() {
                    return Err(format!("{id}: duplicate contract"));
                }
            }
            "Operation" => {
                d.keys(&[
                    "method",
                    "path",
                    "request",
                    "response",
                    "status",
                    "role",
                    "error",
                    "observation",
                    "behavior",
                ])?;
                let op = Operation {
                    method: d.string("method")?,
                    path: d.string("path")?,
                    request: d.iri("request")?,
                    response: d.iri("response")?,
                    status: d.status()?,
                    roles: d.refs("role")?,
                    errors: d.refs("error")?,
                    observations: d.refs("observation")?,
                    behavior: d
                        .values("behavior")
                        .next()
                        .map(|_| d.iri("behavior"))
                        .transpose()?,
                };
                if !matches!(op.method.as_str(), "GET" | "POST")
                    || op.status != 200
                    || op.roles.is_empty()
                    || op.roles.iter().any(|r| {
                        !matches!(
                            r.as_str(),
                            "urn:oddlang:http:provider" | "urn:oddlang:http:consumer"
                        )
                    })
                    || op.errors.is_empty()
                    || op.observations.is_empty()
                {
                    return Err(format!("{id}: invalid operation signature"));
                }
                operations.insert(id.clone(), op);
            }
            "Record" => {
                d.keys(&["field", "constraint"])?;
                records.insert(
                    id.clone(),
                    Record {
                        fields: d.refs("field")?,
                        constraints: d.refs("constraint")?,
                    },
                );
            }
            "Field" => {
                d.keys(&["nativeName", "wireName", "shape", "constraint"])?;
                let f = Field {
                    native_name: d.string("nativeName")?,
                    wire_name: d.string("wireName")?,
                    shape: d.iri("shape")?,
                    constraints: d.refs("constraint")?,
                };
                if !f
                    .native_name
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                    || !f
                        .native_name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || f.wire_name.is_empty()
                {
                    return Err(format!("{id}: invalid field name"));
                }
                fields.insert(id.clone(), f);
            }
            "Required" | "Closed" => {
                d.keys(&[])?;
                constraints.insert(
                    id.clone(),
                    if kind == "Required" {
                        Constraint::Required
                    } else {
                        Constraint::Closed
                    },
                );
            }
            "Bounds" => {
                d.keys(&["min", "max"])?;
                let (min, max) = (d.integer("min")?, d.integer("max")?);
                if min > max {
                    return Err(d.error("min", "exceeds maximum"));
                }
                constraints.insert(id.clone(), Constraint::Bounds { min, max });
            }
            "Error" => {
                d.keys(&["status"])?;
                let status = d.status()?;
                if !(400..600).contains(&status) {
                    return Err(d.error("status", "expected error status"));
                }
                errors.insert(id.clone(), status);
            }
            "LookupObservation" | "TransferObservation" => {
                d.keys(&[])?;
                observations.insert(
                    id.clone(),
                    if kind == "LookupObservation" {
                        Observation::Lookup
                    } else {
                        Observation::Transfer
                    },
                );
            }
            _ => return Err(format!("{id} rdf:type: unsupported {kind}")),
        }
    }
    let (identity, version) = header.ok_or("missing Contract")?;
    let contract = HttpContract {
        entry: String::new(),
        source_files: BTreeMap::new(),
        scenario_files: BTreeSet::new(),
        identity,
        version,
        rule_set_version: 1,
        dependencies,
        operations,
        records,
        fields,
        constraints,
        errors,
        observations,
    };
    contract.validate(ledger_symbols)?;
    Ok(contract)
}

impl HttpContract {
    fn validate(
        &self,
        ledger_symbols: &BTreeMap<Iri, crate::language::Operation>,
    ) -> Result<(), String> {
        if self.operations.is_empty() {
            return Err("missing Operation".into());
        }
        let mut routes = BTreeSet::new();
        for (id, op) in &self.operations {
            let request = self
                .records
                .get(&op.request)
                .ok_or_else(|| format!("{id} {H}request: unresolved {}", op.request))?;
            if !self.records.contains_key(&op.response) {
                return Err(format!("{id} {H}response: unresolved {}", op.response));
            }
            if !op.path.starts_with('/')
                || op.path.split('/').skip(1).any(|s| {
                    s.is_empty()
                        || !(s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                            || (op.method == "GET" && s == "{id}"))
                })
            {
                return Err(format!("{id} {H}path: invalid route"));
            }
            if op.method == "GET"
                && (op.path.matches("{id}").count() != 1
                    || request.fields.len() != 1
                    || !request.fields.iter().any(|f| {
                        self.fields.get(f).is_some_and(|f| {
                            f.wire_name == "id" && f.shape.as_str() == "urn:oddlang:http:Text"
                        })
                    }))
            {
                return Err(format!("{id} {H}path: expected one text id binding"));
            }
            if !routes.insert((&op.method, &op.path)) {
                return Err(format!("{id} {H}path: duplicate route"));
            }
            for e in &op.errors {
                if !self.errors.contains_key(e) {
                    return Err(format!("{id} {H}error: unresolved {e}"));
                }
            }
            for o in &op.observations {
                if !self.observations.contains_key(o) {
                    return Err(format!("{id} {H}observation: unresolved {o}"));
                }
            }
            if let Some(b) = &op.behavior {
                let behavior = ledger_symbols
                    .get(b)
                    .ok_or_else(|| format!("{id} {H}behavior: unresolved {b}"))?;
                for guard in &behavior.guards {
                    if !op.errors.contains(guard.error.as_str()) {
                        return Err(format!(
                            "{id} {H}error: missing mapping for {}",
                            guard.error
                        ));
                    }
                }
            }
        }
        for (id, r) in &self.records {
            if r.constraints.len() != 1
                || !r
                    .constraints
                    .iter()
                    .all(|c| self.constraints.get(c) == Some(&Constraint::Closed))
            {
                return Err(format!("{id} {H}constraint: expected Closed"));
            }
            let (mut native, mut wire) = (BTreeSet::new(), BTreeSet::new());
            for field in &r.fields {
                let f = self
                    .fields
                    .get(field)
                    .ok_or_else(|| format!("{id} {H}field: unresolved {field}"))?;
                if !native.insert(&f.native_name) || !wire.insert(&f.wire_name) {
                    return Err(format!("{id} {H}field: name collision at {field}"));
                }
            }
        }
        for (id, f) in &self.fields {
            if !matches!(
                f.shape.as_str(),
                "urn:oddlang:http:Text" | "urn:oddlang:http:Boolean" | "urn:oddlang:http:I64"
            ) && !self.records.contains_key(&f.shape)
            {
                return Err(format!("{id} {H}shape: unresolved {}", f.shape));
            }
            let (mut required, mut bounds) = (0, 0);
            for c in &f.constraints {
                match self.constraints.get(c) {
                    Some(Constraint::Required) => required += 1,
                    Some(Constraint::Bounds { .. })
                        if f.shape.as_str() == "urn:oddlang:http:I64" =>
                    {
                        bounds += 1
                    }
                    _ => return Err(format!("{id} {H}constraint: invalid {c}")),
                }
            }
            if required != 1 || bounds > 1 {
                return Err(format!(
                    "{id} {H}constraint: expected one Required and at most one Bounds"
                ));
            }
        }
        self.check_reachability()
    }

    pub fn entry(&self) -> &str {
        &self.entry
    }
    pub fn source_files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.source_files
    }
    pub fn scenario_files(&self) -> &BTreeSet<String> {
        &self.scenario_files
    }

    fn check_reachability(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        let mut todo = Vec::new();
        for op in self.operations.values() {
            todo.extend([&op.request, &op.response]);
            todo.extend(op.errors.iter().chain(&op.observations));
        }
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(r) = self.records.get(id) {
                todo.extend(r.fields.iter().chain(&r.constraints));
            }
            if let Some(f) = self.fields.get(id) {
                todo.push(&f.shape);
                todo.extend(&f.constraints);
            }
        }
        let declarations = self
            .records
            .keys()
            .chain(self.fields.keys())
            .chain(self.constraints.keys())
            .chain(self.errors.keys())
            .chain(self.observations.keys());
        if let Some(id) = declarations.filter(|id| !seen.contains(id)).min() {
            return Err(format!("{id}: declaration unreachable from operations"));
        }
        // A required record-valued cycle cannot have a finite JSON witness.
        let mut pending: BTreeMap<_, BTreeSet<_>> = self
            .records
            .iter()
            .map(|(id, r)| {
                (
                    id,
                    r.fields
                        .iter()
                        .map(|f| &self.fields[f].shape)
                        .filter(|s| self.records.contains_key(*s))
                        .collect(),
                )
            })
            .collect();
        while !pending.is_empty() {
            let leaves: BTreeSet<_> = pending
                .iter()
                .filter_map(|(id, deps)| deps.is_empty().then_some(*id))
                .collect();
            if leaves.is_empty() {
                return Err(format!(
                    "{}: cyclic required record shape",
                    pending.first_key_value().unwrap().0
                ));
            }
            pending.retain(|id, deps| {
                deps.retain(|dep| !leaves.contains(dep));
                !leaves.contains(id)
            });
        }
        Ok(())
    }
}
