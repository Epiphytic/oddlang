use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::{
    vocab::{rdf, xsd},
    *,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub const CORE: &str = "urn:oddlang:core:";
pub const LEDGER: &str = "urn:oddlang:ledger:";
pub const ACCOUNT: &str = "urn:oddlang:ledger:Account";
pub const BALANCE: &str = "urn:oddlang:ledger:balance";
pub const ID: &str = "urn:oddlang:ledger:id";
pub const OWNER: &str = "urn:oddlang:ledger:owner";
pub fn core(local: &str) -> NamedNode {
    NamedNode::new_unchecked(format!("{CORE}{local}"))
}
pub(crate) fn named(id: &str) -> NamedOrBlankNode {
    NamedNode::new_unchecked(id).into()
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub iri: String,
    pub rust_name: String,
    pub sql_name: String,
    pub avro_name: String,
    pub aliases: Vec<String>,
    pub range: Option<(i64, i64)>,
}
#[derive(Debug, Clone)]
pub struct Model {
    pub graph: Graph,
    pub imports: BTreeSet<PathBuf>,
    pub fields: BTreeMap<String, Field>,
    pub key: String,
    pub rust_name: String,
    pub sql_name: String,
    pub avro_name: String,
}
type Result<T> = std::result::Result<T, String>;
pub(crate) fn literal(t: TermRef<'_>, datatype: NamedNodeRef<'_>, at: &str) -> Result<String> {
    match t {
        TermRef::Literal(v) if v.datatype() == datatype => Ok(v.value().into()),
        _ => Err(format!("{at}: expected {datatype} literal")),
    }
}
pub(crate) fn iri(t: TermRef<'_>, at: &str) -> Result<String> {
    match t {
        TermRef::NamedNode(n) => Ok(n.as_str().into()),
        _ => Err(format!("{at}: expected named IRI")),
    }
}
pub(crate) fn node(t: TermRef<'_>, at: &str) -> Result<NamedOrBlankNode> {
    match t {
        TermRef::NamedNode(n) => Ok(n.into_owned().into()),
        TermRef::BlankNode(b) => Ok(b.into_owned().into()),
        _ => Err(format!("{at}: expected node")),
    }
}
pub(crate) fn one<'a>(
    g: &'a Graph,
    s: &NamedOrBlankNode,
    p: &str,
    at: &str,
) -> Result<TermRef<'a>> {
    let mut v = g.objects_for_subject_predicate(s, &core(p));
    match (v.next(), v.next()) {
        (Some(t), None) => Ok(t),
        _ => Err(format!("{at}: expected one {p}")),
    }
}
pub(crate) fn allowed(g: &Graph, s: &NamedOrBlankNode, props: &[&str], at: &str) -> Result<()> {
    let bad = g
        .triples_for_subject(s)
        .filter(|t| t.predicate != rdf::TYPE && !props.iter().any(|p| t.predicate == core(p)))
        .map(|t| t.predicate.as_str())
        .min();
    if let Some(p) = bad {
        return Err(format!("{at}: unknown property {p}"));
    }
    Ok(())
}
pub(crate) fn instances(g: &Graph, ty: &str) -> Result<BTreeSet<String>> {
    g.subjects_for_predicate_object(rdf::TYPE, &core(ty))
        .map(|s| match s {
            NamedOrBlankNodeRef::NamedNode(n) => Ok(n.as_str().to_owned()),
            _ => Err(format!("{ty}: declaration requires an IRI")),
        })
        .collect()
}
pub fn load_graph(dir: &Path) -> Result<(Graph, BTreeSet<PathBuf>)> {
    let base = fs::canonicalize(dir).map_err(|e| format!("project directory: {e}"))?;
    let mut graph = Graph::new();
    let mut loaded = BTreeSet::new();
    let mut todo = BTreeSet::from([PathBuf::from("domain.ttl")]);
    while let Some(path) = todo.pop_first() {
        let full = fs::canonicalize(base.join(&path))
            .map_err(|_| format!("unresolved import {}", path.display()))?;
        let path = full
            .strip_prefix(&base)
            .map_err(|_| "import outside project".to_string())?
            .to_owned();
        if !loaded.insert(path.clone()) {
            continue;
        }
        let file = fs::File::open(full).map_err(|e| format!("{}: {e}", path.display()))?;
        let parser = RdfParser::from_format(RdfFormat::Turtle).rename_blank_nodes();
        for q in parser.for_reader(file) {
            let q = q.map_err(|e| format!("{}: {e}", path.display()))?;
            if q.predicate == core("imports") {
                let rel = literal(q.object.as_ref(), xsd::STRING, "imports")?;
                let rel = Path::new(&rel);
                if rel.is_absolute() {
                    return Err("expected relative import".into());
                }
                todo.insert(path.parent().unwrap().join(rel));
            }
            graph.insert(TripleRef::new(&q.subject, &q.predicate, &q.object));
        }
    }
    Ok((graph, loaded))
}
fn checked_name(
    g: &Graph,
    s: &NamedOrBlankNode,
    at: &str,
    target: &str,
    field: bool,
) -> Result<String> {
    let value = literal(one(g, s, "name", at)?, xsd::STRING, at)?;
    let mut bytes = value.bytes();
    if !bytes.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(format!("{at}: invalid identifier {value}"));
    }
    const KEYWORDS: &str = "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while abstract become box do final gen macro override priv try typeof unsized virtual yield";
    let invalid = match target {
        "Rust" => {
            KEYWORDS.split_whitespace().any(|kw| kw == value)
                || if field {
                    value == "new"
                } else {
                    matches!(
                        value.as_str(),
                        "Balance"
                            | "String"
                            | "Result"
                            | "TryFrom"
                            | "Ok"
                            | "Err"
                            | "i64"
                            | "i128"
                            | "str"
                    )
                }
        }
        "Sql" => value.len() > 63,
        _ => false,
    };
    if invalid {
        return Err(format!("{at}: unsupported {target} identifier {value}"));
    }
    Ok(value)
}
/// Check declarations and reject detached RDF that no structural reader consumes.
fn validate_declarations(g: &Graph) -> Result<()> {
    let mut errors = BTreeSet::new();
    // Visit each subject once. Errors identify named declarations, never parser-assigned labels.
    let mut visited = std::collections::HashSet::new();
    for t in g {
        let s = t.subject.into_owned();
        if !visited.insert(s.clone()) {
            continue;
        }
        let owner = match &s {
            NamedOrBlankNode::NamedNode(n) => n.as_str(),
            _ => "anonymous declaration",
        };
        let classes: Vec<_> = g.objects_for_subject_predicate(&s, rdf::TYPE).collect();
        let validate = || -> Result<()> {
            match classes.as_slice() {
                [] => {
                    if g.triples_for_subject(&s)
                        .all(|t| t.predicate == core("imports"))
                    {
                        return Ok(());
                    }
                    let subject_term: Term = s.clone().into();
                    if ["range", "mapping"].iter().any(|p| {
                        g.subjects_for_predicate_object(&core(p), &subject_term)
                            .next()
                            .is_some()
                    }) {
                        return Ok(());
                    }
                    Err(format!("{owner}: unowned declaration"))
                }
                [ty] => {
                    let ty = iri(*ty, owner)?;
                    if !["Entity", "Field", "Projection", "Operation"]
                        .iter()
                        .any(|t| core(t).as_str() == ty)
                    {
                        return Err(format!("{owner}: unknown declaration type {ty}"));
                    }
                    if !matches!(s, NamedOrBlankNode::NamedNode(_)) {
                        return Err(format!("{ty}: declaration requires an IRI"));
                    }
                    if ty == core("Operation").as_str() {
                        allowed(g, &s, &["dependsOn", "imports"], owner)?
                    }
                    Ok(())
                }
                _ => Err(format!("{owner}: expected exactly one declaration type")),
            }
        };
        if let Err(error) = validate() {
            errors.insert(error);
        }
    }
    errors.into_iter().next().map_or(Ok(()), Err)
}
pub fn load_model(project_dir: impl AsRef<Path>) -> Result<Model> {
    let (g, imports) = load_graph(project_dir.as_ref())?;
    if instances(&g, "Entity")? != BTreeSet::from([ACCOUNT.to_owned()]) {
        return Err(format!("expected entity {ACCOUNT}"));
    }
    let account = named(ACCOUNT);
    allowed(&g, &account, &["field", "key"], ACCOUNT)?;
    let key = iri(one(&g, &account, "key", ACCOUNT)?, ACCOUNT)?;
    if key != ID {
        return Err(format!("{ACCOUNT}: expected key {ID}"));
    }
    let refs: BTreeSet<_> = g
        .objects_for_subject_predicate(&account, &core("field"))
        .map(|t| iri(t, ACCOUNT))
        .collect::<Result<_>>()?;
    let declared = instances(&g, "Field")?;
    for id in [ID, OWNER, BALANCE] {
        if !refs.contains(id) {
            return Err(format!("{ACCOUNT}: missing field {id}"));
        }
    }
    if let Some(id) = refs.symmetric_difference(&declared).next() {
        return Err(format!(
            "{id}: field declaration and entity reference differ"
        ));
    }
    let mut fields = BTreeMap::new();
    for id in refs {
        let s = named(&id);
        allowed(&g, &s, &["type", "range"], &id)?;
        let ty = iri(one(&g, &s, "type", &id)?, &id)?;
        let range = if id == BALANCE {
            if ty != core("Integer").as_str() {
                return Err(format!("{id}: expected Integer"));
            }
            let bounds = node(one(&g, &s, "range", &id)?, &id)?;
            allowed(&g, &bounds, &["min", "max"], &format!("{id}/range"))?;
            let bound = |p| -> Result<i64> {
                literal(one(&g, &bounds, p, &id)?, xsd::INTEGER, &id)?
                    .parse()
                    .map_err(|_| format!("{id}/range/{p}: outside signed 64-bit range"))
            };
            let (lo, hi) = (bound("min")?, bound("max")?);
            if lo > hi {
                return Err(format!("{id}: inverted bounds"));
            }
            Some((lo, hi))
        } else {
            if ty != core("Text").as_str()
                || g.objects_for_subject_predicate(&s, &core("range"))
                    .next()
                    .is_some()
            {
                return Err(format!("{id}: expected Text without range"));
            }
            None
        };
        fields.insert(
            id.clone(),
            Field {
                iri: id,
                range,
                rust_name: String::new(),
                sql_name: String::new(),
                avro_name: String::new(),
                aliases: vec![],
            },
        );
    }
    let mut names = BTreeMap::new();
    for id in instances(&g, "Projection")? {
        let s = named(&id);
        allowed(
            &g,
            &s,
            &[
                "imports",
                "target",
                "projects",
                "name",
                "mapping",
                "dependsOn",
            ],
            &id,
        )?;
        if g.objects_for_subject_predicate(&s, &core("target"))
            .next()
            .is_none()
        {
            allowed(&g, &s, &["imports", "dependsOn"], &id)?;
            continue;
        }
        if iri(one(&g, &s, "projects", &id)?, &id)? != ACCOUNT {
            return Err(format!("{id}: unknown entity"));
        }
        let target = iri(one(&g, &s, "target", &id)?, &id)?;
        let target = target.strip_prefix(CORE).unwrap_or("");
        if !["Rust", "Sql", "Avro"].contains(&target) {
            return Err(format!("{id}: unknown target"));
        }
        if names
            .insert(target.to_owned(), checked_name(&g, &s, &id, target, false)?)
            .is_some()
        {
            return Err(format!("{id}: duplicate {target} projection"));
        }
        let mut mappings = BTreeMap::new();
        let mut physical = BTreeSet::new();
        let mut failures = BTreeSet::new();
        for value in g.objects_for_subject_predicate(&s, &core("mapping")) {
            let parse = || -> Result<_> {
                let m = node(value, &id)?;
                let field = iri(one(&g, &m, "field", &id)?, &id)?;
                let at = format!("{id}/mapping/{field}");
                allowed(
                    &g,
                    &m,
                    if target == "Avro" {
                        &["field", "name", "alias"]
                    } else {
                        &["field", "name"]
                    },
                    &at,
                )?;
                let name = checked_name(&g, &m, &at, target, true)?;
                let aliases = g
                    .objects_for_subject_predicate(&m, &core("alias"))
                    .map(|t| literal(t, xsd::STRING, &at))
                    .collect::<Result<BTreeSet<_>>>()?;
                Ok((field, name, aliases))
            };
            match parse() {
                Ok((field, name, aliases)) => {
                    if mappings.insert(field.clone(), (name, aliases)).is_some() {
                        failures.insert(format!("{id}: duplicate mapping {field}"));
                    }
                }
                Err(e) => {
                    failures.insert(e);
                }
            }
        }
        if let Some(e) = failures.into_iter().next() {
            return Err(e);
        }
        for (field, (name, aliases)) in mappings {
            if !physical.insert(name.clone()) {
                return Err(format!("{id}: conflicting physical name {name}"));
            }
            let f = fields
                .get_mut(&field)
                .ok_or_else(|| format!("{id}: unknown field {field}"))?;
            match target {
                "Rust" => f.rust_name = name,
                "Sql" => f.sql_name = name,
                _ => {
                    f.avro_name = name;
                    f.aliases = aliases.into_iter().collect();
                }
            }
        }
    }
    for f in fields.values() {
        if [&f.rust_name, &f.sql_name, &f.avro_name]
            .iter()
            .any(|s| s.is_empty())
        {
            return Err(format!("{}: missing target mapping", f.iri));
        }
    }
    for ty in ["Rust", "Sql", "Avro"] {
        if !names.contains_key(ty) {
            return Err(format!("missing {ty} projection"));
        }
    }
    instances(&g, "Operation")?;
    let dependencies = g
        .triples_for_predicate(&core("dependsOn"))
        .map(|t| iri(t.object, "dependsOn"))
        .collect::<Result<BTreeSet<_>>>()?;
    for dep in dependencies {
        if g.triples_for_subject(&named(&dep)).next().is_none() {
            return Err(format!("dependsOn: unresolved {dep}"));
        }
    }
    validate_declarations(&g)?;
    Ok(Model {
        graph: g,
        imports,
        fields,
        key,
        rust_name: names.remove("Rust").unwrap(),
        sql_name: names.remove("Sql").unwrap(),
        avro_name: names.remove("Avro").unwrap(),
    })
}
