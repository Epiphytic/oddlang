//! Public module type-name graph for one compiler profile.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VOCABULARY: &str = "oddlang-public-type-bindings-v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Observation {
    pub compiler: String,
    pub crate_name: String,
    pub graph: Graph,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub vocabulary: String,
    pub root: String,
    pub modules: BTreeMap<String, Module>,
    pub data: BTreeMap<String, Data>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub path: Vec<String>,
    pub bindings: BTreeMap<String, Binding>,
    pub ambiguous: BTreeMap<String, Ambiguity>,
    pub unresolved: BTreeSet<String>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ambiguity {
    pub primary: Candidate,
    pub alternatives: BTreeSet<Candidate>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub binding: Binding,
    pub audience: Audience,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub target: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Module,
    Data,
    Alias,
    Trait,
    Variant,
    Primitive,
    External,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    pub fields: BTreeMap<String, Field>,
    pub variants: BTreeMap<String, Variant>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub fields: BTreeMap<String, Field>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub audience: Audience,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Audience {
    Public,
    Scope { id: String },
}

impl Graph {
    pub fn canonical_paths(&self) -> BTreeMap<String, Vec<String>> {
        use std::collections::VecDeque;
        // Sorted outgoing edges and a FIFO give the shortest component-wise
        // lexicographic paths without walking descendants of an alias twice.
        let mut paths = BTreeMap::new();
        let mut queue = VecDeque::from([(self.root.clone(), Vec::new())]);
        while let Some((id, path)) = queue.pop_front() {
            if paths.contains_key(&id) {
                continue;
            }
            if let Some(module) = self.modules.get(&id) {
                for (name, binding) in &module.bindings {
                    if binding.kind == Kind::Module {
                        let mut next = path.clone();
                        next.push(name.clone());
                        queue.push_back((binding.target.clone(), next));
                    }
                }
                paths.insert(id, path);
            }
        }
        paths
    }
}
