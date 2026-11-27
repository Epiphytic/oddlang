use super::*;
type Key = (String, u64, String, String);

fn historical_kind(kind: &str) -> Option<&str> {
    Some(match kind {
        "associated-function" => "function",
        "associated-type" => "assoc_type",
        "field" => "struct_field",
        "type-alias" => "type_alias",
        "constant" | "enum" | "function" | "macro" | "module" | "struct" | "variant" => kind,
        _ => return None,
    })
}
fn location(root: &Path, name: &str) -> Option<String> {
    let path = Path::new(name);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    Some(relative.to_str()?.into())
}
pub fn evidence(
    compiler: &Value,
    doc: &Document,
    root: &Path,
    prior: Option<&Value>,
) -> Result<Value> {
    let mut docs = BTreeSet::new();
    for item in &doc.index {
        if item["crate_id"] != 0 {
            continue;
        }
        let kind = item["inner"]
            .as_object()
            .and_then(|o| o.keys().next())
            .ok_or("missing Rustdoc item kind")?;
        if !matches!(
            kind.as_str(),
            "function"
                | "assoc_type"
                | "constant"
                | "enum"
                | "struct_field"
                | "macro"
                | "module"
                | "struct"
                | "type_alias"
                | "variant"
        ) {
            continue;
        }
        let (Some(name), Some(file), Some(line)) = (
            item["name"].as_str().filter(|s| !s.is_empty()),
            item["span"]["filename"].as_str(),
            item["span"]["begin"][0].as_u64(),
        ) else {
            continue;
        };
        if let Some(file) = location(
            root,
            file.strip_prefix("/oddlang-workspace/").unwrap_or(file),
        ) {
            docs.insert((file, line, name.to_owned(), kind.clone()));
        }
    }
    let mut scopes = json!({});
    for (scope, macros) in [("ordinary", false), ("includingExpansions", true)] {
        let mut compiled = BTreeSet::new();
        for row in arr(&compiler["declarations"])? {
            if !macros && row["macroGenerated"] == true {
                continue;
            }
            let (Some(kind), Some(file), Some(line), Some(name)) = (
                historical_kind(txt(&row["kind"])?),
                row["file"].as_str(),
                row["line"].as_u64().filter(|&n| n != 0),
                row["name"].as_str().filter(|s| !s.is_empty()),
            ) else {
                continue;
            };
            if let Some(file) = location(root, file) {
                compiled.insert((file, line, name.into(), kind.into()));
            }
        }
        scopes[scope] = json!({
            "matched":docs.intersection(&compiled).count(),
            "compilerOnly":compiled.difference(&docs).collect::<Vec<_>>(),
            "rustdocOnly":docs.difference(&compiled).collect::<Vec<_>>()
        });
        if let Some(old) = prior {
            if old["compilerCommit"] != COMMIT || old["crate"] != compiler["crateName"] {
                return Err("historical report toolchain or target mismatch".into());
            }
            let mut revisited = Vec::new();
            for side in ["compilerOnly", "rustdocOnly"] {
                for row in arr(&old["matchByScope"][scope][side])? {
                    let key: Key = serde_json::from_value(row.clone())?;
                    let current = match (compiled.contains(&key), docs.contains(&key)) {
                        (true, true) => "matched",
                        (true, false) => "compilerOnly",
                        (false, true) => "rustdocOnly",
                        _ => "absent",
                    };
                    revisited.push(json!({"key":row,"previous":side,"current":current}));
                }
            }
            scopes[scope]["priorKeys"] = json!(revisited);
        }
    }
    Ok(scopes)
}
