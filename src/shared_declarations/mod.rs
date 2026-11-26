//! Project shared constants, named failures, result aliases and string enums.
use oxigraph::model::NamedNode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod cases;
mod rust;
type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Graph {
    format: String,
    id: String,
    #[serde(default)]
    constants: Vec<Constant>,
    #[serde(default)]
    errors: Vec<ErrorType>,
    #[serde(default)]
    results: Vec<Alias>,
    #[serde(default)]
    string_enums: Vec<StringEnum>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Constant {
    id: String,
    value: Value,
}
#[derive(Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
enum Value {
    Literal(u64),
    Reference { r#ref: String },
    Product { multiply: [Box<Value>; 2] },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ErrorType {
    id: String,
    variants: Vec<ErrorVariant>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ErrorVariant {
    id: String,
    payload: Payload,
    message: Vec<Segment>,
}
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Payload {
    None,
    Text,
    U64,
    Bool,
}
#[derive(Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
enum Segment {
    Text { text: String },
    Payload { payload: bool },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    id: String,
    error: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StringEnum {
    id: String,
    variants: Vec<WireVariant>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireVariant {
    id: String,
    wire: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    format: String,
    #[serde(deserialize_with = "unique_map")]
    names: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "unique_map")]
    constant_types: BTreeMap<String, Width>,
    #[serde(default, deserialize_with = "unique_map")]
    variant_order: BTreeMap<String, Vec<String>>,
}
fn unique_map<'de, D, V>(de: D) -> std::result::Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct Keys<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Keys<V> {
        type Value = BTreeMap<String, V>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an IRI map with distinct keys")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut values = BTreeMap::new();
            while let Some((id, value)) = map.next_entry::<String, V>()? {
                if values.contains_key(&id) {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate binding key: {id}"
                    )));
                }
                values.insert(id, value);
            }
            Ok(values)
        }
    }
    de.deserialize_map(Keys(std::marker::PhantomData))
}
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Width {
    U8,
    U16,
    U32,
    U64,
    Usize,
}

impl Graph {
    fn constants(&self) -> Result<BTreeMap<&str, u64>> {
        let expressions: BTreeMap<_, _> = self
            .constants
            .iter()
            .map(|c| (c.id.as_str(), &c.value))
            .collect();
        let mut remaining = BTreeMap::new();
        let mut dependents = BTreeMap::<&str, Vec<&str>>::new();
        let mut ready = Vec::new();
        for (&id, expr) in &expressions {
            let mut refs = BTreeSet::new();
            expr.references(&mut refs);
            for dependency in &refs {
                if !expressions.contains_key(dependency) {
                    return Err(format!("unresolved constant: {dependency}"));
                }
                dependents.entry(dependency).or_default().push(id);
            }
            if refs.is_empty() {
                ready.push(id);
            }
            remaining.insert(id, refs.len());
        }
        let mut values = BTreeMap::new();
        while let Some(id) = ready.pop() {
            let n = expressions[id].value(&values)?;
            values.insert(id, n);
            for &dependent in dependents.get(id).into_iter().flatten() {
                let count = remaining.get_mut(dependent).expect("known dependent");
                *count -= 1;
                if *count == 0 {
                    ready.push(dependent);
                }
            }
        }
        if let Some(id) = remaining.iter().find_map(|(id, n)| (*n != 0).then_some(id)) {
            return Err(format!("cyclic constant dependency: {id}"));
        }
        Ok(values)
    }

    fn validate(&self, binding: &Binding) -> Result<()> {
        if self.format != "oddlang-shared-declarations-v1"
            || binding.format != "oddlang-shared-rust-v1"
        {
            return Err("unknown shared declarations or binding format".into());
        }
        let mut declarations = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut constants = BTreeSet::new();
        let mut groups = BTreeSet::new();
        for id in self
            .constants
            .iter()
            .map(|c| &c.id)
            .chain(self.errors.iter().map(|e| &e.id))
            .chain(self.results.iter().map(|r| &r.id))
            .chain(self.string_enums.iter().map(|e| &e.id))
        {
            let name = binding.name(id)?;
            if !declarations.insert(id) || !names.insert(name) {
                return Err(format!("duplicate declaration or native name: {id}"));
            }
        }
        if declarations.is_empty() {
            return Err("empty shared declarations graph".into());
        }
        for c in &self.constants {
            constants.insert(&c.id);
        }
        for (group, ids) in self
            .errors
            .iter()
            .map(|e| (&e.id, e.variants.iter().map(|v| &v.id).collect::<Vec<_>>()))
            .chain(
                self.string_enums
                    .iter()
                    .map(|e| (&e.id, e.variants.iter().map(|v| &v.id).collect())),
            )
        {
            groups.insert(group);
            let mut variant_names = BTreeSet::new();
            for id in &ids {
                if !declarations.insert(id) || !variant_names.insert(binding.name(id)?) {
                    return Err(format!("duplicate variant IRI or native name: {id}"));
                }
            }
            let order = binding
                .variant_order
                .get(group)
                .ok_or_else(|| format!("missing variant order: {group}"))?;
            if ids.is_empty()
                || order.len() != ids.len()
                || order.iter().collect::<BTreeSet<_>>() != ids.into_iter().collect()
            {
                return Err(format!(
                    "native variant order must cover every variant exactly once: {group}"
                ));
            }
        }
        if declarations.contains(&self.id)
            || NamedNode::new(&self.id).is_err()
            || declarations
                .iter()
                .any(|id| NamedNode::new(id.as_str()).is_err())
        {
            return Err("invalid or reused declaration IRI".into());
        }
        if !binding.names.keys().eq(declarations)
            || !binding.constant_types.keys().eq(constants)
            || !binding.variant_order.keys().eq(groups)
        {
            return Err("binding must cover exactly the declared vocabulary".into());
        }
        for alias in &self.results {
            if !self.errors.iter().any(|err| err.id == alias.error) {
                return Err(format!("unresolved result error: {}", alias.error));
            }
        }
        for error in &self.errors {
            for v in &error.variants {
                if v.message.is_empty() || v.message.iter().any(|seg| matches!(seg, Segment::Payload {payload} if !payload || v.payload == Payload::None)) {
                    return Err(format!("invalid typed display message: {}",v.id));
                }
            }
        }
        for e in &self.string_enums {
            let mut wires = BTreeSet::new();
            if e.variants.iter().any(|v| !wires.insert(&v.wire)) {
                return Err(format!("duplicate wire spelling: {}", e.id));
            }
        }
        Ok(())
    }
}

impl Binding {
    fn name(&self, id: &str) -> Result<&str> {
        let name = self
            .names
            .get(id)
            .ok_or_else(|| format!("unbound declaration: {id}"))?;
        if !name
            .bytes()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
            || !name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
            || name == "gen"
            || syn::parse_str::<syn::Ident>(name).is_err()
        {
            return Err(format!("invalid Rust name: {name}"));
        }
        Ok(name)
    }
}

impl Value {
    fn references<'a>(&'a self, set: &mut BTreeSet<&'a str>) {
        match self {
            Self::Literal(_) => {}
            Self::Reference { r#ref: id } => {
                set.insert(id);
            }
            Self::Product { multiply: [a, b] } => {
                a.references(set);
                b.references(set);
            }
        }
    }
    fn value(&self, values: &BTreeMap<&str, u64>) -> Result<u64> {
        match self {
            Self::Literal(n) => Ok(*n),
            Self::Reference { r#ref: id } => Ok(values[id.as_str()]),
            Self::Product { multiply: [a, b] } => a
                .value(values)?
                .checked_mul(b.value(values)?)
                .ok_or("constant product overflow".into()),
        }
    }
}

pub fn build(graph: &Path, overlay: &Path, output: &Path) -> Result<()> {
    let read = |path: &Path| fs::read(path).map_err(|err| format!("{}: {err}", path.display()));
    let mut graph: Graph =
        serde_json::from_slice(&read(graph)?).map_err(|e| format!("shared declarations: {e}"))?;
    let binding: Binding =
        serde_json::from_slice(&read(overlay)?).map_err(|e| format!("shared Rust binding: {e}"))?;
    graph.constants.sort_by(|a, b| a.id.cmp(&b.id));
    graph.results.sort_by(|a, b| a.id.cmp(&b.id));
    graph.errors.sort_by(|a, b| a.id.cmp(&b.id));
    graph.string_enums.sort_by(|a, b| a.id.cmp(&b.id));
    for e in &mut graph.errors {
        e.variants.sort_by(|a, b| a.id.cmp(&b.id));
    }
    for e in &mut graph.string_enums {
        e.variants.sort_by(|a, b| a.id.cmp(&b.id));
    }
    graph.validate(&binding)?;
    let source = rust::render(&graph, &binding)?;
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let hash =
        |value: &serde_json::Value| digest(&serde_json::to_vec(value).expect("serialize JSON"));
    let obligations =
        serde_json::to_vec_pretty(&cases::derive(&graph, &binding)?).map_err(|e| e.to_string())?;
    let lock = serde_json::json!({"format":"oddlang-shared-projection-v1","domain":graph.id,
        "domainSha256":hash(&serde_json::json!(graph)), "bindingSha256":hash(&serde_json::json!(binding)),
        "artifact":"shared.rs","artifactSha256":digest(source.as_bytes()),"obligationsSha256":digest(&obligations)});
    if output.symlink_metadata().is_ok() {
        return Err("output already exists".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(stage.path().join("shared.rs"), source)
        .and_then(|_| fs::write(stage.path().join("obligations.json"), obligations))
        .and_then(|_| fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n")))
        .map_err(|e| e.to_string())?;
    crate::native_package::publish(stage.path(), output)
}
