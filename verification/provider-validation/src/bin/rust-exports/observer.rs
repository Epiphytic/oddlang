use super::*;
type Modules = BTreeMap<u64, BTreeMap<String, BTreeSet<u64>>>;
fn id(v: &Value) -> Result<u64> {
    v.as_u64().ok_or_else(|| "invalid rustdoc item ID".into())
}
fn kind<'a>(doc: &'a Value, id: u64) -> Option<&'a str> {
    let key = id.to_string();
    doc["index"][&key]["inner"]
        .as_object()
        .and_then(|o| o.keys().next())
        .map(String::as_str)
        .or_else(|| doc["paths"][key]["kind"].as_str())
}
fn doc_exports(doc: &Value) -> Result<(Edges, Vec<Value>)> {
    let index = object(&doc["index"])?;
    let mut modules = Modules::new();
    let mut imports = BTreeMap::<u64, BTreeSet<u64>>::new();
    for item in index.values() {
        let Some(module) = item["inner"].get("module") else {
            continue;
        };
        if item["crate_id"] != 0 {
            continue;
        }
        let key = id(&item["id"])?;
        let named = modules.entry(key).or_default();
        let globs = imports.entry(key).or_default();
        for child in array(&module["items"])? {
            let Some(child) = index.get(&child.to_string()) else {
                continue;
            };
            if child["visibility"] != "public" {
                continue;
            }
            let (name, target) = if let Some(record) = child["inner"].get("use") {
                if record["is_glob"] == true {
                    if let Some(n) = record["id"].as_u64() {
                        globs.insert(n);
                    }
                    continue;
                }
                (&record["name"], &record["id"])
            } else {
                (&child["name"], &child["id"])
            };
            if let (Some(name), Some(n)) = (name.as_str(), target.as_u64())
                && name != "_"
                && matches!(
                    kind(doc, n),
                    Some(
                        "module"
                            | "struct"
                            | "enum"
                            | "union"
                            | "trait"
                            | "trait_alias"
                            | "type_alias"
                            | "variant"
                            | "primitive"
                            | "extern_crate"
                    )
                )
            {
                named.entry(name.to_owned()).or_default().insert(n);
            }
        }
    }
    let explicit = modules.clone();
    let mut ignored = BTreeSet::new();
    loop {
        let mut changed = false;
        for (module, globs) in &imports {
            for target in globs {
                let choices = if let Some(choices) = modules.get(target) {
                    choices.clone()
                } else if kind(doc, *target) == Some("enum")
                    && let Some(e) = index.get(&target.to_string())
                {
                    let mut choices = BTreeMap::new();
                    for v in array(&e["inner"]["enum"]["variants"])? {
                        if let Some(item) = index.get(&v.to_string()) {
                            choices.insert(
                                string(&item["name"])?.to_owned(),
                                BTreeSet::from([id(v)?]),
                            );
                        }
                    }
                    choices
                } else {
                    ignored.insert((*module, *target));
                    continue;
                };
                let names = modules.get_mut(module).unwrap();
                for (name, values) in choices {
                    if explicit[module].contains_key(&name) {
                        continue;
                    }
                    let dest = names.entry(name).or_default();
                    let old = dest.len();
                    dest.extend(values);
                    changed |= old != dest.len();
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut queue = BTreeSet::from([(0, Vec::<String>::new(), id(&doc["root"])?)]);
    let mut canonical = BTreeMap::new();
    let mut exported = Edges::new();
    while let Some((depth, path, key)) = queue.pop_first() {
        if canonical.contains_key(&key) {
            continue;
        }
        canonical.insert(key, path.clone());
        for (name, targets) in modules.get(&key).ok_or("missing rustdoc root module")? {
            let mut edge = path.clone();
            edge.push(name.clone());
            let rows=targets.iter().map(|n|json!({"kind":kind(doc,*n),"path":doc["paths"][n.to_string()]["path"],"target":n})).collect::<Vec<_>>();
            if targets.len() == 1
                && let Some(next) = targets.first()
                && modules.contains_key(next)
            {
                queue.insert((depth + 1, edge.clone(), *next));
            }
            exported.insert(edge, json!(rows));
        }
    }
    let unresolved = ignored
        .iter()
        .filter_map(|(module, n)| {
            canonical
                .get(module)
                .map(|path| json!({"module":path,"target":doc["paths"][n.to_string()]["path"]}))
        })
        .collect();
    Ok((exported, unresolved))
}
fn fields(
    graph: &Value,
    exported: &Edges,
    documented: &Edges,
    doc: &Value,
    cwd: &Path,
    dep: &Path,
) -> Result<Value> {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut gaps = Vec::new();
    for (target, data) in object(&graph["data"])? {
        let mut candidates: Vec<_> = exported
            .iter()
            .filter(|(_, e)| e["kind"] == "data" && e["target"] == target.as_str())
            .map(|(p, _)| p)
            .collect();
        candidates.sort_by_key(|p| (p.len(), *p));
        let mut selected = None;
        'find: for path in candidates {
            if let Some(rows) = documented.get(path) {
                for r in array(rows)? {
                    let kind = string(&r["kind"])?;
                    if matches!(kind, "struct" | "enum" | "union")
                        && let Some(item) = doc["index"].get(r["target"].to_string())
                    {
                        selected = Some((path, kind, &item["inner"][kind]));
                        break 'find;
                    }
                }
            }
        }
        let Some((path, kind, declaration)) = selected else {
            gaps.push(json!({"target":target,"reason":"rustdoc nominal definition absent"}));
            continue;
        };
        if declaration["generics"]["params"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            gaps.push(
                json!({"target":target,"reason":"generic field receiver requires explicit bounds"}),
            );
            continue;
        }
        let receiver = format!("r#observed::{}", spelling(path));
        let mut add = |expression: String, record: &Value| {
            let list = if record["audience"]["kind"] == "public" {
                &mut positive
            } else {
                &mut negative
            };
            list.push(format!(
                "fn read_{}(value: &{receiver}) {{ {expression} }}",
                list.len()
            ));
        };
        for (field, record) in object(&data["fields"])? {
            let name = if field.bytes().all(|c| c.is_ascii_digit()) {
                field.clone()
            } else {
                format!("r#{field}")
            };
            let access = format!("let _ = &value.{name};");
            add(
                if kind == "union" {
                    format!("unsafe {{ {access} }}")
                } else {
                    access
                },
                record,
            );
        }
        for (variant, row) in object(&data["variants"])? {
            for (field, record) in object(&row["fields"])? {
                let pat = if let Ok(i) = field.parse::<usize>() {
                    format!("{receiver}::r#{variant}({}member, ..)", "_, ".repeat(i))
                } else {
                    format!("{receiver}::r#{variant} {{ r#{field}: member, .. }}")
                };
                add(
                    format!("if let {pat} = value {{ let _ = member; }}"),
                    record,
                );
            }
        }
    }
    if !positive.is_empty() {
        require(compile(cwd, dep, &positive.join("\n"))?)?;
    }
    for code in &negative {
        let out = compile(cwd, dep, code)?;
        if out.status.success()
            || !diagnostics(&out)?
                .iter()
                .any(|d| d["code"]["code"] == "E0616")
        {
            return Err(format!("expected E0616: {}", String::from_utf8_lossy(&out.stderr)).into());
        }
    }
    Ok(json!({"publicReads":positive.len(),"privateRejections":negative.len(),"gaps":gaps}))
}
pub fn profile(
    lock: &Path,
    package: &str,
    name: &str,
    triple: &str,
    features: &[String],
    default: bool,
) -> Result<String> {
    let adapter = installed_adapter()?;
    let fields = json!({"adapter":adapter,"adapterInputs":["Cargo.toml","Cargo.lock","driver.rs","shapes.rs","shape_model.rs","capture.rs","exports.rs","export_model.rs"],
        "package":package,"targetKind":"lib","targetName":name,"targetTriple":triple,"features":features,"defaultFeatures":default,"compilerCommit":PIN,"cargoLockSha256":sha(lock)?});
    Ok(object(&fields)?
        .iter()
        .map(|(k, v)| format!("{k} = {v}\n"))
        .collect())
}
pub fn probe(args: &[String]) -> Result {
    let [cli, manifest, package, out, options @ ..] = args else {
        return Err("expected ODDLANG MANIFEST PACKAGE OUT".into());
    };
    let manifest = Path::new(manifest).canonicalize()?;
    let parent = manifest.parent().unwrap();
    let cli = Path::new(cli).canonicalize()?;
    let ver = version()?;
    assert!(ver.contains(&format!("commit-hash: {PIN}\n")));
    let host = ver
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("missing host")?;
    let mut name = package.replace('-', "_");
    let mut default = false;
    let mut features = Vec::new();
    let mut options = options.iter();
    while let Some(o) = options.next() {
        match o.as_str() {
            "--target-name" => name = options.next().ok_or("missing target name")?.clone(),
            "--target" => {
                if options.next().ok_or("missing target")? != host {
                    return Err(
                        "independent field and import probes require the matching native target"
                            .into(),
                    );
                }
            }
            "--feature" => features.push(options.next().ok_or("missing feature")?.clone()),
            "--default-features" => default = true,
            _ => return Err(format!("unknown option {o}").into()),
        }
    }
    let out = absolute(out)?;
    if let Some(p) = out.parent() {
        fs::create_dir_all(p)?;
    }
    fs::create_dir(&out)?;
    let scratch = tempfile::tempdir()?;
    let temp = scratch.path();
    let meta = metadata(&manifest, &env::current_dir()?)?;
    let workspace = Path::new(string(&meta["workspace_root"])?);
    let p = temp.join("profile.toml");
    fs::write(
        &p,
        profile(
            &workspace.join("Cargo.lock"),
            package,
            &name,
            host,
            &features,
            default,
        )?,
    )?;
    for dest in [out.join("recovered"), temp.join("recovered-again")] {
        checked(
            cmd(&cli)
                .args(["recover", "rust-exports"])
                .arg(&manifest)
                .arg("--profile")
                .arg(&p)
                .arg("--out")
                .arg(dest),
        )?;
    }
    for file in ["exports.json", "exports.lock"] {
        assert_eq!(
            fs::read(out.join("recovered").join(file))?,
            fs::read(temp.join("recovered-again").join(file))?
        );
    }
    let opts = |tool: &str| {
        let mut c = cmd("cargo");
        c.current_dir(parent)
            .args([
                tool,
                "--locked",
                "--offline",
                "--release",
                "--manifest-path",
            ])
            .arg(&manifest)
            .args(["--package", package, "--target", host, "--lib"]);
        if !default {
            c.arg("--no-default-features");
        }
        for f in &features {
            c.args(["--features", f]);
        }
        c
    };
    let native = checked(
        opts("check")
            .arg("--target-dir")
            .arg(out.join("native"))
            .arg("--message-format=json"),
    )?;
    checked(
        opts("rustdoc")
            .env("RUSTC_BOOTSTRAP", "1")
            .arg("--target-dir")
            .arg(out.join("rustdoc"))
            .args(["--", "-Z", "unstable-options", "--output-format", "json"]),
    )?;
    let mut dep = None;
    for line in std::str::from_utf8(&native)?.lines() {
        let row: Value = serde_json::from_str(line)?;
        if row["reason"] == "compiler-artifact"
            && row["target"]["name"] == name
            && let Some(file) = array(&row["filenames"])?
                .iter()
                .filter_map(Value::as_str)
                .find(|f| f.ends_with(".rmeta"))
        {
            dep = Some(PathBuf::from(file));
            break;
        }
    }
    let dep = dep.ok_or("missing library metadata")?;
    let src = out.join("recovered/exports.json");
    let g = read(&src)?;
    let edges = bindings(&g, "bindings")?;
    let ambiguous = bindings(&g, "ambiguous")?;
    let mut source = String::from("#![deny(ambiguous_glob_imports)]\n");
    for (i, path) in edges.keys().enumerate() {
        source.push_str(&format!(
            "pub use r#observed::{} as witness_{i};\n",
            spelling(path)
        ));
    }
    require(compile(temp, &dep, &source)?)?;
    let doc = read(
        &out.join("rustdoc")
            .join(host)
            .join("doc")
            .join(format!("{name}.json")),
    )?;
    let (doc_edges, globs) = doc_exports(&doc)?;
    let mut differences = Vec::new();
    for path in edges
        .keys()
        .chain(doc_edges.keys())
        .chain(ambiguous.keys())
        .collect::<BTreeSet<_>>()
    {
        let row = edges.get(path);
        let listed = doc_edges.get(path).cloned().unwrap_or(json!([]));
        let disp = if ambiguous.contains_key(path) {
            json!("compiler-ambiguity")
        } else if row.is_none() {
            let r = compile(temp, &dep, &imports(path))?;
            if r.status.success() {
                json!("rustdoc-only-resolves")
            } else {
                json!({"rustdoc-only-rejected":diagnostics(&r)?.iter().map(|d|&d["message"]).collect::<Vec<_>>()})
            }
        } else if array(&listed)?.is_empty() {
            json!("compiler-and-caller-only")
        } else {
            let target = string(&row.unwrap()["target"])?;
            let normalized = target
                .split_once("::")
                .map_or(target, |(_, v)| v)
                .replace("::{type}", "");
            if array(&listed)?
                .iter()
                .filter_map(|r| paths(&r["path"]).ok())
                .any(|p| p.join("::") == normalized)
            {
                continue;
            }
            json!("both-resolve-origin-path-difference")
        };
        differences.push(json!({"spelling":path,"disposition":disp,"compiler":row.or_else(||ambiguous.get(path)),"rustdoc":listed}));
    }
    let field_probes = fields(&g, &edges, &doc_edges, &doc, temp, &dep)?;
    write(
        &out.join("evidence.json"),
        &json!({"compiler":PIN,"target":host,"package":package,"features":features,"defaultFeatures":default,"exportsSha256":sha(&src)?,
        "moduleCount":object(&g["modules"])?.len(),"compilePositiveCount":edges.len(),"rustdocSpellingCount":doc_edges.len(),"rustdocFormat":doc["format_version"],"ambiguityCount":ambiguous.len(),"rustdocUnresolvedGlobs":globs,"unresolvedCompilerSpellings":unresolved(&g)?,"discrepancies":differences,"deterministic":true,"fieldProbes":field_probes}),
    )?;
    println!(
        "{package} {} {} {}",
        edges.len(),
        doc_edges.len(),
        differences.len()
    );
    Ok(())
}
