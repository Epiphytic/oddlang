//! Directional compatibility and declaration-level reverse dependencies.
use super::*;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Compatible,
    Incompatible,
    Unknown,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReport {
    pub old_client_new_provider: Verdict,
    pub new_client_old_provider: Verdict,
    pub changes: Vec<Change>,
}
#[derive(Serialize)]
pub struct Change {
    pub declaration: String,
    pub kind: String,
    pub before: Value,
    pub after: Value,
}

fn declarations(c: &HttpContract) -> BTreeMap<(String, String), Value> {
    let mut values = BTreeMap::new();
    let mut add = |id: &Iri, kind: &str, value: Value| {
        values.insert((id.to_string(), kind.into()), value);
    };
    for (id, f) in &c.fields {
        add(id, "wireName", json!(f.wire_name));
        add(id, "field", json!([f.native_name, f.shape, f.constraints]));
    }
    for (id, v) in &c.records {
        add(id, "record", json!(v));
    }
    for (id, v) in &c.operations {
        add(id, "operation", json!(v));
    }
    for (id, v) in &c.constraints {
        add(id, "constraint", json!(v));
    }
    for (id, v) in &c.errors {
        add(id, "error", json!(v));
    }
    for (id, v) in &c.observations {
        add(id, "observation", json!(v));
    }
    add(
        &c.identity,
        "contract",
        json!([c.rule_set_version, c.dependencies]),
    );
    values
}

pub fn compatibility(old: &HttpContract, new: &HttpContract) -> CompatibilityReport {
    let a = declarations(old);
    let b = declarations(new);
    let mut changes = Vec::new();
    let mut verdict = Verdict::Compatible;
    for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        if a.get(k) == b.get(k) {
            continue;
        }
        let (id, kind) = k;
        let required_rename = kind == "wireName"
            && [old, new].iter().all(|c| {
                c.fields.get(id.as_str()).is_some_and(|f| {
                    f.constraints
                        .iter()
                        .any(|k| c.constraints.get(k) == Some(&Constraint::Required))
                })
            });
        if required_rename {
            verdict = Verdict::Incompatible;
        } else if verdict == Verdict::Compatible {
            verdict = Verdict::Unknown;
        }
        changes.push(Change {
            declaration: id.clone(),
            kind: kind.clone(),
            before: a.get(k).cloned().unwrap_or(Value::Null),
            after: b.get(k).cloned().unwrap_or(Value::Null),
        });
    }
    CompatibilityReport {
        old_client_new_provider: verdict,
        new_client_old_provider: verdict,
        changes,
    }
}

pub fn affected(c: &HttpContract, changed: &[Iri]) -> Vec<Obligation> {
    let mut reverse: BTreeMap<&Iri, Vec<&Iri>> = BTreeMap::new();
    for (id, f) in &c.fields {
        for used in f.constraints.iter().chain([&f.shape]) {
            reverse.entry(used).or_default().push(id);
        }
    }
    for (id, r) in &c.records {
        for used in r.fields.iter().chain(&r.constraints) {
            reverse.entry(used).or_default().push(id);
        }
    }
    for (id, o) in &c.operations {
        for used in o
            .errors
            .iter()
            .chain(&o.observations)
            .chain(o.behavior.iter())
            .chain([&o.request, &o.response])
        {
            reverse.entry(used).or_default().push(id);
        }
    }
    let mut pending = changed.iter().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if seen.insert(id.as_str()) {
            pending.extend(reverse.get(id).into_iter().flatten());
        }
    }
    inventory(c)
        .into_iter()
        .filter(|o| seen.contains(o.declaration.as_str()) || seen.contains(o.operation.as_str()))
        .collect()
}
