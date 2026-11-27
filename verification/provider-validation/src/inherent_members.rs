//! Public names of concrete local inherent implementations in Rustdoc JSON.
use crate::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn inventory(document: &Value, owners: &BTreeMap<u64, Vec<String>>) -> Result<Value> {
    let index = document["index"]
        .as_object()
        .ok_or("missing rustdoc index")?;
    let item = |id: &Value| index.get(&id.to_string()).ok_or("missing rustdoc item");
    let mut excluded = BTreeMap::<&str, usize>::new();
    let mut declarations = Vec::new();
    for row in index.values() {
        let Some(implementation) = row["inner"].get("impl") else {
            continue;
        };
        if row["crate_id"] != 0 || !implementation["trait"].is_null() {
            continue;
        }
        let receiver = &implementation["for"]["resolved_path"];
        let Some(owner) = receiver["id"].as_u64().and_then(|id| owners.get(&id)) else {
            *excluded
                .entry("unresolved-or-nonnominal-receiver")
                .or_default() += 1;
            continue;
        };
        let nominal = item(&receiver["id"])?["inner"]
            .as_object()
            .and_then(|map| map.values().next())
            .ok_or("expected nominal item")?;
        let has_params = |v: &Value| v.as_array().is_some_and(|a| !a.is_empty());
        if !receiver["args"].is_null()
            || has_params(&nominal["generics"]["params"])
            || has_params(&implementation["generics"]["params"])
            || has_params(&implementation["generics"]["where_predicates"])
        {
            *excluded
                .entry("generic-or-specialized-receiver")
                .or_default() += 1;
            continue;
        }
        for id in implementation["items"]
            .as_array()
            .ok_or("missing impl items")?
        {
            let member = item(id)?;
            if member["visibility"] != "public" {
                continue;
            }
            let kind = member["inner"]
                .as_object()
                .and_then(|o| o.keys().next())
                .ok_or("missing impl item kind")?;
            let name = member["name"].as_str().unwrap_or("");
            if !matches!(kind.as_str(), "function" | "assoc_const" | "assoc_type")
                || name.is_empty()
            {
                return Err(
                    format!("unsupported public inherent item kind or name: {kind}").into(),
                );
            }
            declarations.push((owner, kind, name));
        }
    }
    declarations.sort();
    let items = declarations
        .into_iter()
        .map(|(owner, kind, name)| json!({"owner":owner,"kind":kind,"name":name}))
        .collect::<Vec<_>>();
    Ok(
        json!({"scope":"indexed-local-inherent-impls-with-private-and-hidden-items-under-cfg-doc","items":items,"excludedIndexedLocalInherentImpls":excluded}),
    )
}
