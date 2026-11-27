use super::*;
fn observe(path: &Path) -> Result<(Edges, BTreeMap<Spelling, Vec<Spelling>>)> {
    let g = read(path)?;
    let edges = bindings(&g, "bindings")?;
    let mut rows = Edges::new();
    let mut families = BTreeMap::<String, Vec<Spelling>>::new();
    for (name, e) in edges {
        let target = string(&e["target"])?;
        let mut fields = BTreeMap::new();
        if let Some(data) = g["data"].get(target) {
            for (f, info) in object(&data["fields"])? {
                fields.insert(f.clone(), info["audience"]["kind"].clone());
            }
            for (v, row) in object(&data["variants"])? {
                for (f, info) in object(&row["fields"])? {
                    fields.insert(format!("{v}::{f}"), info["audience"]["kind"].clone());
                }
            }
        }
        if e["kind"] != "primitive" {
            families
                .entry(target.into())
                .or_default()
                .push(name.clone());
        }
        rows.insert(name, json!({"kind":e["kind"],"fieldAudiences":fields}));
    }
    let mut aliases = BTreeMap::new();
    for family in families.into_values() {
        for alias in &family {
            aliases.insert(alias.clone(), family.clone());
        }
    }
    Ok((rows, aliases))
}
pub fn report(old: &Path, new: &Path, out: &Path) -> Result {
    let (before, a) = observe(old)?;
    let (after, b) = observe(new)?;
    let x = before.keys().collect::<BTreeSet<_>>();
    let y = after.keys().collect::<BTreeSet<_>>();
    let mut changed = Vec::new();
    for n in x.intersection(&y).copied() {
        if before[n] != after[n] || a.get(n) != b.get(n) {
            changed.push(json!({"spelling":n,"original":before[n],"generated":after[n],"originalAliases":a.get(n),"generatedAliases":b.get(n)}));
        }
    }
    let report = json!({"originalSha256":sha(old)?,"generatedSha256":sha(new)?,"profiles":"public module type edges and field reading at the canonical cut",
        "sharedCount":x.intersection(&y).count(),"originalOnly":x.difference(&y).collect::<Vec<_>>(),"generatedOnly":y.difference(&x).collect::<Vec<_>>(),"changedAtSharedSpelling":changed});
    write(out, &report)?;
    println!(
        "{} {} {} {}",
        x.intersection(&y).count(),
        x.difference(&y).count(),
        y.difference(&x).count(),
        changed.len()
    );
    Ok(())
}
pub fn reconcile(compiler: &Path, doc: &Path) -> Result {
    let c = read(compiler)?;
    let d = read(doc)?;
    let kinds = BTreeMap::from([
        ("associated-function", "function"),
        ("associated-type", "assoc_type"),
        ("constant", "constant"),
        ("enum", "enum"),
        ("field", "struct_field"),
        ("function", "function"),
        ("macro", "macro"),
        ("module", "module"),
        ("struct", "struct"),
        ("type-alias", "type_alias"),
        ("variant", "variant"),
    ]);
    let defs = array(&d["definitions"])?;
    if !defs
        .iter()
        .any(|v| v["path"].get(0) == Some(&c["crateName"]))
    {
        return Err("Rustdoc has no path rooted at the compiler's crate name".into());
    }
    let mut expected = BTreeSet::new();
    for r in defs {
        let s = &r["span"];
        let (Some(name), Some(kind)) = (r["name"].as_str(), r["kind"].as_str()) else {
            continue;
        };
        if !name.is_empty() && s["file"][0] == "workspace" && kinds.values().any(|k| *k == kind) {
            expected.insert((
                string(&s["file"][1])?.to_owned(),
                s["begin"][0].as_u64().ok_or("missing line")?,
                name.to_owned(),
                kind.to_owned(),
            ));
        }
    }
    let mut scopes = json!({});
    let root = Path::new(string(&d["workspaceRoot"])?);
    for (scope, expanded) in [("ordinary", false), ("includingExpansions", true)] {
        let mut actual = BTreeSet::new();
        for r in array(&c["declarations"])? {
            let (Some(kind), Some(file), Some(line), Some(name)) = (
                r["kind"].as_str().and_then(|k| kinds.get(k)),
                r["file"].as_str(),
                r["line"].as_u64(),
                r["name"].as_str(),
            ) else {
                continue;
            };
            if file.is_empty()
                || line == 0
                || name.is_empty()
                || (!expanded && r["macroGenerated"] == true)
            {
                continue;
            }
            let p = Path::new(file);
            let relative = if p.is_absolute() {
                let Ok(p) = p.strip_prefix(root) else {
                    continue;
                };
                p
            } else {
                p
            };
            actual.insert((
                relative.to_string_lossy().into_owned(),
                line,
                name.to_owned(),
                kind.to_string(),
            ));
        }
        scopes[scope] = json!({"matched":actual.intersection(&expected).count(),"compilerOnly":actual.difference(&expected).collect::<Vec<_>>(),"rustdocOnly":expected.difference(&actual).collect::<Vec<_>>()});
    }
    let report = json!({"scope":"Declaration overlap by workspace path, line, name and normalized kind","compilerCommit":c["compiler"],"rustdocCompiler":d["compiler"],"crate":c["crateName"],"matchByScope":scopes});
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
