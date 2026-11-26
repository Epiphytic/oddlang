use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::{
    Literal, NamedNode, NamedOrBlankNode, Term,
    vocab::{rdf, xsd},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const PREFIX: &str = "urn:oddlang:wasm:";
pub(crate) type Res<T> = Result<T, String>;
pub(crate) type Map = BTreeMap<String, String>;

#[derive(Default, Debug, Clone)]
pub(crate) struct Node {
    pub kind: String,
    pub name: String,
    pub props: Map,
    pub links: BTreeMap<String, BTreeSet<String>>,
}
impl Node {
    pub(crate) fn interface_path(&self) -> &str {
        self.props
            .get("from")
            .map(String::as_str)
            .unwrap_or(&self.name)
    }
    pub fn targets(&self, key: &str) -> impl Iterator<Item = &String> {
        self.links.get(key).into_iter().flatten()
    }
    pub fn link(&mut self, key: &str, id: &str) {
        self.links.entry(key.into()).or_default().insert(id.into());
    }
}

#[derive(Clone, Debug)]
pub struct World {
    pub(crate) nodes: BTreeMap<String, Node>,
    pub(crate) package: String,
    pub(crate) selected: String,
}

fn allowed(kind: &str, key: &str) -> bool {
    key == "label"
        || match kind {
            "World" => matches!(key, "entry" | "import" | "export" | "include" | "use"),
            "Interface" => matches!(key, "type" | "function" | "use" | "from"),
            "Function" => matches!(key, "param" | "result"),
            "Record" => key == "field",
            "Variant" | "Enum" => key == "case",
            "Field" | "Case" | "Param" => matches!(key, "ordinal" | "valueType" | "min" | "max"),
            "ForeignType" => key == "from",
            _ => false,
        }
}
fn link_kind(key: &str, kind: &str) -> bool {
    match key {
        "include" => kind == "World",
        "type" => matches!(kind, "Record" | "Enum" | "Variant"),
        "use" => matches!(kind, "Record" | "Enum" | "Variant" | "ForeignType"),
        "import" | "export" => matches!(kind, "Function" | "Interface"),
        "function" => kind == "Function",
        "param" => kind == "Param",
        "field" => kind == "Field",
        "case" => kind == "Case",
        _ => false,
    }
}
impl World {
    pub fn from_turtle(bytes: &[u8]) -> Res<Self> {
        let mut nodes = BTreeMap::<String, Node>::new();
        for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(bytes) {
            let q = quad.map_err(|e| format!("invalid Wasm ontology: {e}"))?;
            let NamedOrBlankNode::NamedNode(id) = q.subject else {
                return Err("stable declaration IRI required".into());
            };
            let n = nodes.entry(id.as_str().to_owned()).or_default();
            let (key, value) = if q.predicate == rdf::TYPE {
                let Term::NamedNode(k) = q.object else {
                    return Err("expected declaration class".into());
                };
                (
                    "class",
                    k.as_str()
                        .strip_prefix(PREFIX)
                        .ok_or("unsupported Wasm class")?
                        .to_owned(),
                )
            } else {
                let key = q
                    .predicate
                    .as_str()
                    .strip_prefix(PREFIX)
                    .ok_or("unsupported Wasm predicate")?;
                if key == "class" {
                    return Err("unsupported Wasm predicate class".into());
                }
                match q.object {
                    Term::NamedNode(v) => {
                        n.link(key, v.as_str());
                        continue;
                    }
                    Term::Literal(v) => {
                        let datatype = match key {
                            "ordinal" | "min" | "max" => xsd::INTEGER,
                            "entry" => xsd::BOOLEAN,
                            _ => xsd::STRING,
                        };
                        if v.datatype() != datatype {
                            return Err(format!("invalid literal type for {key}"));
                        }
                        (key, v.value().to_owned())
                    }
                    _ => return Err("stable declaration IRI required".into()),
                }
            };
            if n.props
                .insert(key.into(), value.clone())
                .is_some_and(|old| old != value)
            {
                return Err(format!("multiple {key} values for {id}"));
            }
        }
        for (id, n) in &mut nodes {
            n.name = n
                .props
                .remove("name")
                .ok_or_else(|| format!("missing name for {id}"))?;
            n.kind = n
                .props
                .remove("class")
                .ok_or_else(|| format!("missing class for {id}"))?;
            if !matches!(
                n.kind.as_str(),
                "Package"
                    | "Interface"
                    | "World"
                    | "Function"
                    | "Record"
                    | "Enum"
                    | "Variant"
                    | "ForeignType"
                    | "Field"
                    | "Param"
                    | "Case"
            ) {
                return Err(format!("unsupported Wasm construct {}", n.kind));
            }
            for k in n.props.keys() {
                if !allowed(&n.kind, k)
                    || matches!(
                        k.as_str(),
                        "import"
                            | "export"
                            | "use"
                            | "type"
                            | "include"
                            | "field"
                            | "case"
                            | "param"
                            | "function"
                    )
                {
                    return Err(format!("invalid {k} for {id}"));
                }
            }
            if n.kind != "Package" {
                wit_parser::validate_id(&n.name)
                    .map_err(|_| format!("invalid WIT name {}", n.name))?;
            }
        }
        let packages: Vec<_> = nodes
            .iter()
            .filter(|(_, n)| n.kind == "Package")
            .map(|(id, _)| id.clone())
            .collect();
        let [package] = packages.as_slice() else {
            return Err("expected one Wasm package".into());
        };
        let worlds: Vec<_> = nodes.iter().filter(|(_, n)| n.kind == "World").collect();
        let selected = match worlds.as_slice() {
            [(id, _)] => id.to_string(),
            _ => {
                let entries: Vec<_> = worlds
                    .iter()
                    .filter(|(_, n)| n.props.get("entry").is_some_and(|s| s == "true"))
                    .collect();
                let [(id, _)] = entries.as_slice() else {
                    return Err("expected one entry world".into());
                };
                id.to_string()
            }
        };
        let w = Self {
            nodes,
            package: package.clone(),
            selected,
        };
        w.validate()?;
        Ok(w)
    }
    pub fn identity(&self) -> &str {
        &self.package
    }
    pub fn name(&self) -> &str {
        &self.nodes[&self.selected].name
    }
    pub fn version(&self) -> &str {
        self.nodes[&self.package].name.rsplit_once('@').unwrap().1
    }
    pub fn package_name(&self) -> &str {
        &self.nodes[&self.package].name
    }
    fn validate(&self) -> Res<()> {
        let (base, v) = self
            .package_name()
            .split_once('@')
            .ok_or("versioned WIT package required")?;
        let (ns, pkg) = base.split_once(':').ok_or("invalid WIT package")?;
        if !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
            || v.is_empty()
        {
            return Err("invalid WIT package version".into());
        }
        for n in [ns, pkg] {
            wit_parser::validate_id(n).map_err(|_| "invalid WIT package")?
        }
        let mut global = BTreeSet::new();
        let mut owners = BTreeMap::<&str, usize>::new();
        for n in self.nodes.values() {
            for (key, children) in &n.links {
                if matches!(
                    key.as_str(),
                    "param" | "case" | "field" | "type" | "function" | "use" | "import" | "export"
                ) {
                    for child in children {
                        if key != "use"
                            || self
                                .nodes
                                .get(child)
                                .is_some_and(|n| n.kind == "ForeignType")
                        {
                            *owners.entry(child).or_default() += 1;
                        }
                    }
                }
            }
        }
        for (id, n) in &self.nodes {
            let owner_count = owners.get(id.as_str()).copied().unwrap_or_default();
            if matches!(
                n.kind.as_str(),
                "Record" | "Variant" | "Enum" | "Case" | "Field" | "Param"
            ) && owner_count != 1
            {
                return Err(format!("expected one declaring owner for {id}"));
            }
            if matches!(n.kind.as_str(), "ForeignType" | "Function") && owner_count == 0 {
                return Err(format!("unreferenced declaration {id}"));
            }
            if matches!(n.kind.as_str(), "Interface" | "World") && !global.insert(&n.name) {
                return Err(format!("WIT name collision {}", n.name));
            }
            for (k, targets) in &n.links {
                let mut names = BTreeSet::new();
                for target in targets {
                    let t = self
                        .nodes
                        .get(target)
                        .ok_or_else(|| format!("unresolved Wasm IRI {target}"))?;
                    if !allowed(&n.kind, k) || !link_kind(k, &t.kind) {
                        return Err(format!("invalid {k} edge on {id}"));
                    }
                    if !names.insert(&t.name) {
                        return Err(format!("WIT name collision {}", t.name));
                    }
                }
            }
            for key in ["param", "field", "case"] {
                self.ordered(n, key)?;
            }
            for key in ["valueType", "result"] {
                if let Some(s) = n.props.get(key) {
                    super::wit::type_expr(s)?;
                }
            }
            if matches!(n.kind.as_str(), "Field" | "Param") && !n.props.contains_key("valueType") {
                return Err(format!("missing valueType for {id}"));
            }
            if n.kind == "Enum"
                && n.targets("case")
                    .any(|c| self.nodes[c].props.contains_key("valueType"))
            {
                return Err("enum case cannot carry a payload".into());
            }
            if n.kind == "ForeignType" || n.kind == "Interface" && n.props.contains_key("from") {
                let path = n
                    .props
                    .get("from")
                    .ok_or("missing foreign interface path")?;
                super::wit::external(path)?;
                if n.kind == "Interface"
                    && path
                        .split_once('/')
                        .and_then(|(_, p)| p.split_once('@'))
                        .map(|(name, _)| name)
                        != Some(n.name.as_str())
                {
                    return Err(format!("foreign interface name disagrees with {path}"));
                }
            }
            let mut bounds = [None, None];
            for (i, k) in ["min", "max"].into_iter().enumerate() {
                if let Some(s) = n.props.get(k) {
                    let x = s
                        .parse::<i64>()
                        .map_err(|_| format!("invalid {k} for {id}"))?;
                    let valid = match n.props.get("valueType").map(String::as_str) {
                        Some("s8") => i8::try_from(x).is_ok(),
                        Some("s16") => i16::try_from(x).is_ok(),
                        Some("s32") => i32::try_from(x).is_ok(),
                        Some("s64") => true,
                        _ => false,
                    };
                    if !valid {
                        return Err(format!("invalid {k} for {id}"));
                    }
                    bounds[i] = Some(x);
                }
            }
            if let [Some(a), Some(b)] = bounds
                && a > b
            {
                return Err(format!("inverted bounds for {id}"));
            }
        }
        Ok(())
    }
    pub(crate) fn ordered<'a>(&'a self, n: &'a Node, key: &str) -> Res<Vec<&'a String>> {
        let mut ordered = BTreeMap::new();
        for id in n.targets(key) {
            let ordinal = self.nodes[id]
                .props
                .get("ordinal")
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or_else(|| format!("invalid or missing ordinal for {id}"))?;
            if ordered.insert(ordinal, id).is_some() {
                return Err(format!("duplicate {key} ordinal {ordinal}"));
            }
        }
        if ordered.keys().copied().ne(0..ordered.len()) {
            return Err(format!("noncontiguous {key} ordinals"));
        }
        Ok(ordered.into_values().collect())
    }
    pub(crate) fn effective(&self) -> Res<BTreeMap<String, BTreeSet<String>>> {
        self.effective_world(&self.selected)
    }
    pub(crate) fn effective_world(&self, world: &str) -> Res<BTreeMap<String, BTreeSet<String>>> {
        fn walk(
            w: &World,
            id: &str,
            stack: &mut BTreeSet<String>,
            out: &mut BTreeMap<String, BTreeSet<String>>,
        ) -> Res<()> {
            if !stack.insert(id.into()) {
                return Err("cyclic WIT world include".into());
            }
            let n = &w.nodes[id];
            for inc in n.targets("include") {
                walk(w, inc, stack, out)?
            }
            for k in ["import", "export", "use"] {
                out.entry(k.into())
                    .or_default()
                    .extend(n.targets(k).cloned());
            }
            stack.remove(id);
            Ok(())
        }
        let mut out = BTreeMap::new();
        walk(self, world, &mut BTreeSet::new(), &mut out)?;
        Ok(out)
    }
    pub fn identities(&self) -> Res<Map> {
        fn visit(w: &World, id: &str, path: String, out: &mut Map) -> Res<()> {
            if let Some(old) = out.insert(path.clone(), id.into())
                && old != id
            {
                return Err(format!("WIT name collision {path}"));
            }
            for k in ["function", "type", "param", "field", "case", "use"] {
                for child in w.nodes[id].targets(k) {
                    visit(w, child, format!("{path}/{k}/{}", w.nodes[child].name), out)?;
                }
            }
            Ok(())
        }
        let mut out = Map::from([
            ("package".into(), self.package.clone()),
            ("world".into(), self.selected.clone()),
        ]);
        for (id, n) in &self.nodes {
            if n.kind == "Interface" {
                visit(
                    self,
                    id,
                    format!("interface/{}", n.interface_path()),
                    &mut out,
                )?
            }
        }
        for (k, ids) in self.effective()? {
            for id in ids {
                visit(
                    self,
                    &id,
                    format!("world/{k}/{}", self.nodes[&id].name),
                    &mut out,
                )?;
            }
        }
        Ok(out)
    }
    pub fn behavior_roles(&self) -> Res<BTreeSet<(String, String)>> {
        let mut out = BTreeSet::new();
        for (k, ids) in self.effective()? {
            if k == "use" {
                continue;
            }
            let role = format!("wasm.{k}er");
            for id in ids {
                let n = &self.nodes[&id];
                if n.kind == "Function" {
                    out.insert((id, role.clone()));
                } else {
                    out.extend(n.targets("function").map(|f| (f.clone(), role.clone())));
                }
            }
        }
        Ok(out)
    }
    pub fn to_turtle(&self) -> Res<String> {
        let mut out = format!("@prefix w: <{PREFIX}> .\n");
        for (id, n) in &self.nodes {
            let subject = NamedNode::new(id).map_err(|e| e.to_string())?;
            out += &format!(
                "{subject} a w:{} ; w:name {} ",
                n.kind,
                Literal::new_simple_literal(&n.name)
            );
            for (k, v) in &n.props {
                let lit = match k.as_str() {
                    "ordinal" | "min" | "max" => Literal::new_typed_literal(v, xsd::INTEGER),
                    "entry" => Literal::new_typed_literal(v, xsd::BOOLEAN),
                    _ => Literal::new_simple_literal(v),
                };
                out += &format!("; w:{k} {lit} ");
            }
            for (k, ids) in &n.links {
                for id in ids {
                    out += &format!(
                        "; w:{k} {} ",
                        NamedNode::new(id).map_err(|e| e.to_string())?
                    )
                }
            }
            out += ".\n";
        }
        Ok(out)
    }
}
