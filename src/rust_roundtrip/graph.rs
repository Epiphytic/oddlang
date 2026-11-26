use super::{FileSyntax, Result, Sources, Syntax, VERSION, parser};
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::{GraphName, Literal, NamedNode, NamedOrBlankNode, Term, Triple, vocab::xsd},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

const NS: &str = "urn:oddlang:rust-tree:";

fn file_id(path: &str) -> String {
    let mut id = format!("{NS}file:");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            id.push(char::from(byte));
        } else {
            write!(id, "%{byte:02X}").unwrap();
        }
    }
    id
}

fn triple(id: &str, field: &str, value: impl Into<Term>) -> Triple {
    Triple::new(
        NamedNode::new_unchecked(id),
        NamedNode::new_unchecked(format!("{NS}{field}")),
        value,
    )
}

fn sorted_bytes(lines: impl IntoIterator<Item = String>) -> Vec<u8> {
    let mut lines: Vec<_> = lines.into_iter().collect();
    lines.sort_unstable();
    let mut bytes = Vec::new();
    for line in lines {
        bytes.extend(line.as_bytes());
        bytes.extend(b" .\n");
    }
    bytes
}

pub(super) fn encode(sources: &Sources, files: &BTreeMap<String, FileSyntax>) -> Vec<u8> {
    let mut lines = Vec::new();
    let mut add = |row: Triple| lines.push(row.to_string());
    let bundle = format!("{NS}bundle");
    add(triple(&bundle, "version", Literal::from(VERSION)));
    add(triple(
        &bundle,
        "package",
        Literal::from(sources.package.as_str()),
    ));
    add(triple(
        &bundle,
        "edition",
        Literal::from(sources.edition.as_str()),
    ));
    for (path, file) in files {
        let id = file_id(path);
        add(triple(&bundle, "file", NamedNode::new_unchecked(&id)));
        add(triple(&id, "path", Literal::from(path.as_str())));
        add(triple(&id, "bom", Literal::from(file.bom)));
        for &offset in &file.crlf {
            add(triple(&id, "crlf", Literal::from(offset as u64)));
        }
        for (index, node) in file.nodes.iter().enumerate() {
            let subject = format!("{id}#{index}");
            match node {
                Syntax::Branch { kind, children } => {
                    add(triple(&subject, "kind", Literal::from(kind.as_str())));
                    for (position, index) in children.iter().enumerate() {
                        add(triple(
                            &subject,
                            &format!("child:{position}"),
                            NamedNode::new_unchecked(format!("{id}#{index}")),
                        ));
                    }
                }
                Syntax::Leaf {
                    kind,
                    lexical_kind,
                    spelling,
                } => {
                    for (key, value) in [
                        ("kind", kind),
                        ("lexicalKind", lexical_kind),
                        ("spelling", spelling),
                    ] {
                        add(triple(&subject, key, Literal::from(value.as_str())));
                    }
                }
            }
        }
    }
    sorted_bytes(lines)
}

type Predicates = BTreeMap<String, Vec<Term>>;

pub(super) struct Graph {
    rows: BTreeMap<String, Predicates>,
    bytes: Vec<u8>,
}

impl Graph {
    pub(super) fn read(bytes: &[u8]) -> Result<Self> {
        let mut rows: BTreeMap<String, Predicates> = BTreeMap::new();
        let mut lines = BTreeSet::new();
        for row in RdfParser::from_format(RdfFormat::NTriples).for_slice(bytes) {
            let row = row.map_err(|error| format!("syntax.ttl: {error}"))?;
            let NamedOrBlankNode::NamedNode(subject) = row.subject else {
                return Err("syntax graph subjects must be named".into());
            };
            if row.graph_name != GraphName::DefaultGraph {
                return Err("unexpected named graph".into());
            }
            let field = row
                .predicate
                .as_str()
                .strip_prefix(NS)
                .ok_or("unexpected syntax predicate")?;
            if !lines.insert(
                Triple::new(subject.clone(), row.predicate.clone(), row.object.clone()).to_string(),
            ) {
                return Err("duplicate syntax triple".into());
            }
            rows.entry(subject.into_string())
                .or_default()
                .entry(field.into())
                .or_default()
                .push(row.object);
        }
        Ok(Self {
            rows,
            bytes: sorted_bytes(lines),
        })
    }

    pub(super) fn canonical(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn emit(&self, path: &str, edition: &str) -> Result<String> {
        let id = file_id(path);
        let file = self.rows.get(&id).ok_or("missing syntax file")?;
        if string(file, "path")? != path {
            return Err("syntax file path disagrees with inventory".into());
        }
        let bom = match literal(file, "bom", xsd::BOOLEAN)?.value() {
            "true" => true,
            "false" => false,
            _ => return Err("invalid BOM flag".into()),
        };
        let offsets: BTreeSet<usize> = file
            .get("crlf")
            .into_iter()
            .flatten()
            .map(|term| integer(term))
            .collect::<Result<_>>()?;

        let mut text = String::new();
        let node_prefix = format!("{id}#");
        let mut pending = vec![0];
        let mut preorder = 0;
        while let Some(index) = pending.pop() {
            if index != preorder {
                return Err("invalid syntax preorder, shared child or cycle".into());
            }
            preorder += 1;
            let subject = format!("{node_prefix}{index}");
            let node = self.rows.get(&subject).ok_or("missing syntax node")?;
            string(node, "kind")?;
            let children = child_indices(node, &node_prefix)?;
            if node.contains_key("spelling") {
                if !children.is_empty() {
                    return Err("syntax leaf has children".into());
                }
                let spelling = string(node, "spelling")?;
                parser::validate_leaf(spelling, string(node, "lexicalKind")?, edition)?;
                text.push_str(spelling);
            } else {
                pending.extend(children.into_iter().rev());
            }
        }
        restore(text, bom, offsets)
    }
}

fn child_indices(node: &Predicates, prefix: &str) -> Result<Vec<usize>> {
    let mut children = BTreeMap::new();
    for (key, objects) in node {
        let Some(position) = key.strip_prefix("child:") else {
            continue;
        };
        let [Term::NamedNode(target)] = objects.as_slice() else {
            return Err("child edge must have one named target".into());
        };
        let position: usize = position.parse().map_err(|_| "invalid child position")?;
        let target: usize = target
            .as_str()
            .strip_prefix(prefix)
            .ok_or("cross-file child")?
            .parse()
            .map_err(|_| "invalid child identity")?;
        if children.insert(position, target).is_some() {
            return Err("duplicate child position".into());
        }
    }
    if children.keys().copied().ne(0..children.len()) {
        return Err("child positions must be contiguous from zero".into());
    }
    Ok(children.into_values().collect())
}

fn literal<'a>(
    node: &'a Predicates,
    key: &str,
    datatype: oxigraph::model::NamedNodeRef<'_>,
) -> Result<&'a Literal> {
    if let Some([Term::Literal(value)]) = node.get(key).map(Vec::as_slice)
        && value.datatype() == datatype
    {
        return Ok(value);
    }
    Err(format!("missing or invalid {key} literal"))
}

fn string<'a>(node: &'a Predicates, key: &str) -> Result<&'a str> {
    Ok(literal(node, key, xsd::STRING)?.value())
}

fn integer(term: &Term) -> Result<usize> {
    if let Term::Literal(value) = term
        && value.datatype() == xsd::INTEGER
    {
        return value
            .value()
            .parse()
            .map_err(|_| "invalid CRLF offset".into());
    }
    Err("invalid CRLF offset".into())
}

fn restore(text: String, bom: bool, offsets: BTreeSet<usize>) -> Result<String> {
    let mut raw = String::new();
    if bom {
        raw.push('\u{feff}');
    }
    let mut start = 0;
    for offset in offsets {
        if text.as_bytes().get(offset) != Some(&b'\n') {
            return Err("CRLF offset does not identify a normalized line feed".into());
        }
        raw.push_str(&text[start..offset]);
        raw.push('\r');
        start = offset;
    }
    raw.push_str(&text[start..]);
    Ok(raw)
}
