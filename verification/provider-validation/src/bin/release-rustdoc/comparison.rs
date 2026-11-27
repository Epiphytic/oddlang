use super::*;

fn kind(raw: &str) -> Option<&'static str> {
    Some(match raw {
        "crate" => "package",
        "module" => "module",
        "struct" => "record",
        "union" => "union",
        "enum" => "sum",
        "variant" => "variant",
        "field" | "struct_field" => "field",
        "trait" => "interface",
        "macro" => "macro",
        "function" | "associated-function" => "operation",
        "associated-type" | "assoc_type" => "associated-type",
        "type-alias" | "type_alias" => "type-alias",
        "constant" => "constant",
        "associated-constant" | "assoc_const" => "constant",
        "static" => "static",
        _ => return None,
    })
}
#[derive(Debug)]
struct Row<'a> {
    id: String,
    parent: Option<String>,
    name: Option<&'a str>,
    raw: &'a str,
    kind: Option<&'static str>,
    file: Option<&'a str>,
    line: &'a Value,
    visibility: &'a Value,
}
type Key = (Vec<String>, String);
type Rows<'a> = BTreeMap<String, Row<'a>>;

fn id(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
fn optional(v: &Value) -> Option<String> {
    (!v.is_null()).then(|| id(v))
}

fn compiler_rows(c: &Value) -> Result<Vec<Row<'_>>> {
    arr(&c["declarations"])?
        .iter()
        .map(|r| {
            let raw = txt(&r["kind"])?;
            Ok(Row {
                id: id(&r["id"]),
                parent: optional(&r["owner"]),
                name: r["name"].as_str(),
                raw,
                kind: kind(raw),
                file: r["file"].as_str(),
                line: &r["line"],
                visibility: &r["visibility"],
            })
        })
        .collect()
}

fn members<'a>(raw: &str, data: &'a Value) -> &'a [Value] {
    let children = match raw {
        "module" | "trait" | "impl" => &data["items"],
        "enum" => &data["variants"],
        "union" => &data["fields"],
        "struct" | "variant" => {
            if let Some((form, body)) = data["kind"].as_object().and_then(|o| o.iter().next()) {
                if form == "tuple" {
                    body
                } else {
                    &body["fields"]
                }
            } else {
                &Value::Null
            }
        }
        _ => &Value::Null,
    };
    children.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn doc_rows(d: &Document) -> Result<Vec<Row<'_>>> {
    let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rows = Vec::new();
    for row in d.index.iter().filter(|r| r["crate_id"] == 0) {
        let (raw, payload) = row["inner"]
            .as_object()
            .and_then(|o| o.iter().next())
            .ok_or("missing Rustdoc item kind")?;
        let key = id(&row["id"]);
        for child in members(raw, payload).iter().filter(|v| !v.is_null()) {
            parents.entry(id(child)).or_default().insert(key.clone());
        }
        rows.push(Row {
            id: key,
            parent: None,
            name: row["name"].as_str(),
            raw,
            kind: if row["id"] == d.root {
                Some("package")
            } else {
                kind(raw)
            },
            file: row["span"]["filename"].as_str(),
            line: &row["span"]["begin"][0],
            visibility: &row["visibility"],
        });
    }
    for row in &mut rows {
        row.parent = parents
            .get(&row.id)
            .filter(|p| p.len() == 1)
            .and_then(|p| p.first().cloned());
    }
    Ok(rows)
}

fn path(rows: &Rows, key: &str) -> Option<Vec<String>> {
    let mut row = rows.get(key)?;
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    loop {
        if !seen.insert(&row.id) {
            return None;
        }
        names.push(row.name.filter(|n| !n.is_empty())?.to_owned());
        if row.kind? == "package" {
            names.reverse();
            return Some(names);
        }
        row = rows.get(row.parent.as_ref()?)?;
    }
}

struct Observed<'a> {
    rows: Rows<'a>,
    mapped: BTreeMap<Key, Vec<String>>,
    omitted: BTreeMap<String, usize>,
}
impl<'a> Observed<'a> {
    fn new(list: Vec<Row<'a>>) -> Self {
        let order: Vec<_> = list.iter().map(|r| r.id.clone()).collect();
        let rows: Rows = list.into_iter().map(|r| (r.id.clone(), r)).collect();
        let mut mapped: BTreeMap<Key, Vec<String>> = BTreeMap::new();
        let mut omitted = BTreeMap::new();
        for key in order {
            let row = &rows[&key];
            if let Some(names) = path(&rows, &key) {
                mapped
                    .entry((names, row.kind.unwrap().into()))
                    .or_default()
                    .push(key);
            } else {
                *omitted.entry(row.raw.into()).or_default() += 1;
            }
        }
        Self {
            rows,
            mapped,
            omitted,
        }
    }
    fn spans(&self, key: &Key, root: &Path) -> Vec<Value> {
        self.mapped
            .get(key)
            .into_iter()
            .flatten()
            .map(|id| {
                let r = &self.rows[id];
                let file = r
                    .file
                    .map(|f| f.strip_prefix("/oddlang-workspace/").unwrap_or(f))
                    .map(|f| f.strip_prefix(&format!("{}/", root.display())).unwrap_or(f));
                json!({"file":file,"line":r.line})
            })
            .collect()
    }
    fn within(&self, id: &str, item: &[String]) -> Value {
        if !self
            .rows
            .get(id)
            .is_some_and(|r| matches!(r.kind, Some("package" | "module")))
        {
            return Value::Null;
        }
        path(&self.rows, id)
            .filter(|p| p.len() < item.len() && item.starts_with(p))
            .map(|p| json!({"within":p}))
            .unwrap_or(Value::Null)
    }
}

pub fn owners(document: &Document) -> Result<BTreeMap<u64, Vec<String>>> {
    let rows = Observed::new(doc_rows(document)?);
    let mut owners = BTreeMap::new();
    for r in rows
        .rows
        .values()
        .filter(|r| matches!(r.kind, Some("record" | "sum" | "union")))
    {
        if let Some(p) = path(&rows.rows, &r.id) {
            owners.insert(r.id.parse()?, p);
        }
    }
    Ok(owners)
}

fn access(compiler: &Value, left: &Observed, right: &Observed) -> Result<Value> {
    let mut restrictions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in arr(&compiler["edges"])?
        .iter()
        .filter(|e| e["relation"] == "visibility-scope")
    {
        restrictions
            .entry(id(&edge["from"]))
            .or_default()
            .push(id(&edge["to"]));
    }
    let mut result = json!({"agreement":[],"different":[],"unresolved":[]});
    let mut outside = BTreeMap::new();
    for (key, ids) in &left.mapped {
        let Some(doc_ids) = right.mapped.get(key) else {
            continue;
        };
        if ids.len() != 1 || doc_ids.len() != 1 {
            continue;
        }
        let (names, kind) = key;
        let cid = &ids[0];
        let c = &left.rows[cid];
        let d = &right.rows[&doc_ids[0]];
        let cp = c.parent.as_ref().and_then(|i| left.rows.get(i));
        let dp = d.parent.as_ref().and_then(|i| right.rows.get(i));
        let scope = cp.and_then(|r| r.kind);
        let module = matches!(scope, Some("module" | "package"))
            && matches!(
                kind.as_str(),
                "module"
                    | "record"
                    | "union"
                    | "sum"
                    | "interface"
                    | "operation"
                    | "static"
                    | "constant"
                    | "type-alias"
            );
        if cp.is_none()
            || dp.is_none()
            || scope != dp.and_then(|r| r.kind)
            || !(module || kind == "field" && matches!(scope, Some("record" | "union")))
        {
            *outside.entry(kind).or_insert(0) += 1;
            continue;
        }
        let whole = json!({"within":[names[0]]});
        let cv = match c.visibility.as_str() {
            Some("public") => json!("public"),
            Some("crate") => whole.clone(),
            Some("private") => left.within(&cp.unwrap().id, names),
            Some("restricted") => restrictions
                .get(cid)
                .filter(|v| v.len() == 1)
                .map(|v| left.within(&v[0], names))
                .unwrap_or(Value::Null),
            _ => Value::Null,
        };
        let dv = match d.visibility.as_str() {
            Some("public") => json!("public"),
            Some("crate") => whole,
            _ => right.within(&id(&d.visibility["restricted"]["parent"]), names),
        };
        let status = if cv.is_null() || dv.is_null() {
            "unresolved"
        } else if cv == dv {
            "agreement"
        } else {
            "different"
        };
        result[status]
            .as_array_mut()
            .unwrap()
            .push(json!({"path":names,"kind":kind,"compiler":cv,"rustdoc":dv}));
    }
    result["outsideScopeMatchedByKind"] = json!(outside);
    Ok(result)
}

pub fn compare(c: &Value, d: &Document, root: &Path) -> Result<Value> {
    let left = Observed::new(compiler_rows(c)?);
    let right = Observed::new(doc_rows(d)?);
    let mut groups = json!({"matched":[],"compilerOnly":[],"rustdocOnly":[],"ambiguous":[]});
    for key in left
        .mapped
        .keys()
        .chain(right.mapped.keys())
        .collect::<BTreeSet<_>>()
    {
        let a = left.spans(key, root);
        let b = right.spans(key, root);
        let status = if a.len() > 1 || b.len() > 1 {
            "ambiguous"
        } else if a.is_empty() {
            "rustdocOnly"
        } else if b.is_empty() {
            "compilerOnly"
        } else {
            "matched"
        };
        groups[status]
            .as_array_mut()
            .unwrap()
            .push(json!({"path":key.0,"kind":key.1,"compiler":a,"rustdoc":b}));
    }
    groups["unkeyedCompilerByKind"] = json!(left.omitted);
    groups["unkeyedRustdocByKind"] = json!(right.omitted);
    groups["declaredAccess"] = access(c, &left, &right)?;
    Ok(groups)
}
