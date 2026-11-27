use crate::trace::{Producers, Roots, identity};
use crate::{Result, read, sha};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};

// Rustdoc can name unavailable compiler and dependency sources. Resolve the
// existing prefix and normalize the remaining components for their logical ID.
fn resolve_span(path: &Path) -> Result<std::path::PathBuf> {
    if let Ok(existing) = path.canonicalize() {
        return Ok(existing);
    }
    let mut resolved = std::path::PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                resolved.pop();
            }
            other => {
                resolved.push(other.as_os_str());
                if let Ok(real) = resolved.canonicalize() {
                    resolved = real;
                }
            }
        }
    }
    Ok(resolved)
}

pub fn definitions(
    doc: &Value,
    roots: &Roots,
    producers: &Producers,
    workspace: &Path,
) -> Result<Vec<Value>> {
    if doc["format_version"] != 61 {
        return Err("nightly rustdoc JSON version changed".into());
    }
    let paths = doc["paths"].as_object().ok_or("missing Rustdoc paths")?;
    let local: Vec<_> = doc["index"]
        .as_object()
        .ok_or("missing Rustdoc index")?
        .values()
        .filter(|i| i["crate_id"] == 0)
        .collect();
    let id = |item: &Value| item["id"].as_u64().ok_or("invalid Rustdoc ID");
    let local_ids: BTreeSet<_> = local
        .iter()
        .map(|i| id(i))
        .collect::<std::result::Result<_, _>>()?;
    fn references(value: &Value, refs: &mut BTreeSet<(u64, String)>) -> Result<()> {
        match value {
            Value::Object(o) => {
                if let Some(path) = o.get("resolved_path").filter(|v| v.is_object()) {
                    refs.insert((
                        path["id"].as_u64().ok_or("missing path id")?,
                        path["path"].as_str().ok_or("missing path spelling")?.into(),
                    ));
                }
                for v in o.values() {
                    references(v, refs)?;
                }
            }
            Value::Array(a) => {
                for v in a {
                    references(v, refs)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut rows = Vec::new();
    for item in local {
        let i = id(item)?;
        let (kind, body) = item["inner"]
            .as_object()
            .and_then(|o| o.iter().next())
            .ok_or("empty Rustdoc definition")?;
        let mut refs = BTreeSet::new();
        references(body, &mut refs)?;
        let refs: Vec<_> = refs
            .into_iter()
            .map(|(id, name)| {
                let target = paths.get(&id.to_string());
                let status = if target.is_some() {
                    "qualified-path"
                } else if local_ids.contains(&id) {
                    "local-id"
                } else {
                    "opaque-rustdoc-id"
                };
                json!({
                    "spelling": name,
                    "rustdocId": id,
                    "qualifiedPath": target.map(|v| &v["path"]),
                    "status": status
                })
            })
            .collect();
        let span = &item["span"];
        let span = if span.is_null() {
            Value::Null
        } else {
            let name = span["filename"]
                .as_str()
                .ok_or("missing Rustdoc span filename")?;
            let path = Path::new(name);
            let absolute = workspace.join(path.strip_prefix("/oddlang-workspace").unwrap_or(path));
            let path = resolve_span(&absolute)?;
            let file = identity(&path, roots, producers)?
                .unwrap_or_else(|| ["external-span".into(), name.into()]);
            json!({"file":file,"begin":span["begin"],"end":span["end"]})
        };
        rows.push(json!({
            "rustdocId": i,
            "name": item["name"],
            "kind": kind,
            "span": span,
            "path": paths.get(&i.to_string()).map(|v| &v["path"]),
            "signatureTypeReferences": refs,
            "macroExpansionOrigin": "unavailable"
        }));
    }
    rows.sort_by_key(|r| r["rustdocId"].as_u64());
    Ok(rows)
}

pub fn bundle(bundle: &Path, pin: &str, root: &Path) -> Result<Value> {
    let lock = bundle.join("bundle.lock");
    if sha(&lock)? != pin {
        return Err("unexpected syntax bundle lock digest".into());
    }
    let layer = read(&lock)?;
    let inventory = bundle.join("sources.json");
    if layer["version"] != 1
        || layer["syntaxSha256"] != sha(&bundle.join("syntax.ttl"))?
        || layer["sourcesSha256"] != sha(&inventory)?
    {
        return Err("syntax bundle content does not match lock".into());
    }
    let files = read(&inventory)?;
    let files = files["files"]
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or("syntax bundle has an empty source inventory")?;
    for (name, expected) in files {
        let rel = Path::new(name);
        let path = root.join(rel);
        let normalized = rel.components().all(|c| matches!(c, Component::Normal(_)))
            && rel.components().collect::<std::path::PathBuf>().as_os_str() == rel.as_os_str();
        if !normalized || path.canonicalize().ok().as_ref() != Some(&path) || !path.is_file() {
            return Err(format!("invalid syntax bundle source {name}").into());
        }
        if sha(&path)? != *expected {
            return Err(format!("stale syntax bundle source digest: {name}").into());
        }
    }
    Ok(
        json!({"lockSha256":pin,"syntaxSha256":layer["syntaxSha256"],"sourcesSha256":layer["sourcesSha256"],"sourcesMatched":files.len()}),
    )
}

pub fn compare_stable(report: &Value, stable: &Value) -> Result<Value> {
    fn inputs(rows: &Value) -> Result<BTreeSet<Vec<String>>> {
        let mut set = BTreeSet::new();
        for row in rows.as_array().ok_or("missing compiler input rows")? {
            let values: Vec<Vec<String>> = serde_json::from_value(row["inputs"].clone())?;
            set.extend(values);
        }
        Ok(set)
    }
    let a = inputs(&stable["builds"]["check"]["rustcInputs"])?;
    let b = inputs(&report["compilerInvocations"])?;
    let scope = |set: &BTreeSet<Vec<String>>| {
        set.iter()
            .filter(|v| v.first().is_some_and(|s| s == "workspace"))
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let workspace = scope(&a);
    if workspace.is_empty() || workspace != scope(&b) {
        return Err("stable and nightly pinned package source identities differ".into());
    }
    Ok(
        json!({"stableScopeEntries":workspace.len(),"entireInputClosureMatches":a==b,"stableAndNightlyPinnedSourceIdentitiesMatch":true,
        "onlyStableInputs":a.difference(&b).collect::<Vec<_>>(),"onlyNightlyInputs":b.difference(&a).collect::<Vec<_>>()}),
    )
}
