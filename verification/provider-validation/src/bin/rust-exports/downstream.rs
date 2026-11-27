use super::*;
fn locked(src: &Path) -> Result<(Value, Value, String)> {
    let g = read(src)?;
    let path = src.with_file_name("exports.lock");
    let lock = read(&path)?;
    if lock["exportsSha256"] != sha(src)?
        || g["vocabulary"] != "oddlang-public-type-bindings-v1"
        || lock["vocabulary"] != g["vocabulary"]
        || lock["pin"]["targetKind"] != "lib"
    {
        return Err("expected digest-pinned library type graph".into());
    }
    Ok((g, lock, sha(&path)?))
}
fn configs(pin: &Value, entry: &Path) -> Result {
    let mut actual = BTreeMap::new();
    let mut collect = |role: &str, dir: &Path| -> Result {
        for name in ["config", "config.toml"] {
            let p = dir.join(name);
            if p.exists() {
                actual.insert(format!("{role}/{name}"), sha(&p)?);
            }
        }
        Ok(())
    };
    for (role, dir) in [
        ("project", entry.parent().unwrap().to_owned()),
        ("driver", env::current_dir()?.canonicalize()?),
    ] {
        for (i, p) in dir.ancestors().enumerate() {
            collect(&format!("{role}-{i}"), &p.join(".cargo"))?;
        }
    }
    if let Some(home) = env::var_os("CARGO_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
    {
        collect("cargo-home", &home)?;
    }
    if json!(actual) != pin["cargoConfigSha256"] {
        return Err("Cargo configuration differs from subject export lock".into());
    }
    Ok(())
}
pub fn probe(expected: &Path, subject: &Path, manifest: &Path, cache: &Path) -> Result<Value> {
    let src = subject.join("exports.json");
    let (g, lock, lockhash) = locked(&src)?;
    let (reference, ref_lock, ref_hash) = locked(expected)?;
    let pin = &lock["pin"];
    if pin["packageManifestSha256"] != sha(manifest)? {
        return Err("package manifest differs from subject export lock".into());
    }
    for k in ["compilerCommit", "targetTriple"] {
        if pin[k] != ref_lock["pin"][k] {
            return Err("reference and subject need the same compiler and target".into());
        }
    }
    let mut entry = None;
    for parent in manifest.parent().unwrap().ancestors() {
        let path = parent.join("Cargo.toml");
        if path.is_file() && pin["manifestSha256"] == sha(&path)? {
            entry = Some(path);
            break;
        }
    }
    let entry = entry.ok_or("workspace entry manifest differs from subject export lock")?;
    configs(pin, &entry)?;
    let ver = version()?;
    for (k, key) in [("commit-hash", "compilerCommit"), ("host", "targetTriple")] {
        if !ver.contains(&format!("{k}: {}\n", string(&pin[key])?)) {
            return Err("native compiler profile differs from subject lock".into());
        }
    }
    let cwd = entry.parent().unwrap();
    let meta = metadata(manifest, cwd)?;
    let workspace = Path::new(string(&meta["workspace_root"])?);
    let pkg = array(&meta["packages"])?
        .iter()
        .find(|p| p["manifest_path"].as_str() == manifest.to_str())
        .ok_or("missing package")?;
    if pkg["name"] != pin["package"]
        || sha(&workspace.join("Cargo.lock"))? != pin["cargoLockSha256"]
    {
        return Err("package selection or Cargo.lock differs from subject lock".into());
    }
    if let Some(revision) = pin["sourceRevision"].as_str() {
        let actual = checked(
            cmd("git")
                .current_dir(workspace)
                .args(["rev-parse", "HEAD"]),
        )?;
        let changes = checked(cmd("git").current_dir(workspace).args([
            "status",
            "--porcelain",
            "--untracked-files=all",
        ]))?;
        if std::str::from_utf8(&actual)?.trim() != revision || !changes.is_empty() {
            return Err("source revision differs from subject export lock".into());
        }
    }
    let mut c = cmd("cargo");
    c.current_dir(cwd)
        .args(["check", "--manifest-path"])
        .arg(manifest)
        .args([
            "--locked",
            "--offline",
            "--lib",
            "--release",
            "--target",
            string(&pin["targetTriple"])?,
            "--package",
            string(&pin["package"])?,
        ]);
    if pin["defaultFeatures"] == false {
        c.arg("--no-default-features");
    }
    for f in paths(&pin["features"])? {
        c.args(["--features", &f]);
    }
    let rows = checked(
        c.arg("--target-dir")
            .arg(cache)
            .arg("--message-format=json"),
    )?;
    let mut files = Vec::new();
    for line in std::str::from_utf8(&rows)?.lines() {
        let r: Value = serde_json::from_str(line)?;
        if r["reason"] == "compiler-artifact"
            && r["package_id"] == pkg["id"]
            && r["target"]["name"] == pin["targetName"]
            && array(&r["target"]["kind"])?.contains(&json!("lib"))
        {
            files.extend(
                paths(&r["filenames"])?
                    .into_iter()
                    .filter(|p| p.ends_with(".rmeta")),
            );
        }
    }
    let [file] = files.as_slice() else {
        return Err("expected one selected library metadata artifact".into());
    };
    let dep = Path::new(file);
    let temp = tempfile::tempdir()?;
    let edges = bindings(&reference, "bindings")?;
    let known = bindings(&g, "bindings")?;
    let amb = bindings(&reference, "ambiguous")?;
    let subject_amb = bindings(&g, "ambiguous")?;
    let unresolved_subject = unresolved(&g)?;
    let mut rows = Vec::new();
    for p in edges.keys().chain(amb.keys()).collect::<BTreeSet<_>>() {
        let out = compile(
            temp.path(),
            dep,
            &format!("#![deny(ambiguous_glob_imports)]\n{}", imports(p)),
        )?;
        let diagnostics = diagnostics(&out)?;
        let codes = diagnostics
            .iter()
            .filter(|d| d["level"] == "error")
            .filter_map(|d| d["code"]["code"].as_str())
            .collect::<BTreeSet<_>>();
        let ok = out.status.success();
        if !ok
            && (codes.is_empty()
                || codes
                    .iter()
                    .any(|s| !matches!(*s, "E0432" | "E0433" | "E0603" | "ambiguous_glob_imports")))
        {
            return Err(format!(
                "unexpected downstream diagnostic for {p:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            )
            .into());
        }
        if !ok && known.contains_key(p) {
            return Err(format!("subject's own public binding failed to compile: {p:?}").into());
        }
        let mut row = json!({"spelling":p,"expectedKind":edges.get(p).map(|e|&e["kind"]).unwrap_or(&json!("ambiguous")),"subjectKind":known.get(p).map(|e|&e["kind"]),"importCompiles":ok,"errorCodes":codes});
        if let Some(v) = amb.get(p) {
            row["expectedAmbiguity"] = v.clone();
        }
        if let Some(v) = subject_amb.get(p) {
            row["subjectAmbiguity"] = v.clone();
        }
        if unresolved_subject.contains(p) {
            row["subjectUnresolved"] = json!(true);
        }
        rows.push(row);
    }
    require(compile(temp.path(), dep, "pub use observed as subject;")?)?;
    Ok(
        json!({"expectedExportsSha256":sha(expected)?,"expectedLockSha256":ref_hash,"expectedUnresolved":unresolved(&reference)?,
        "subjectLockSha256":lockhash,"subjectPackage":pin["package"],"subjectTargetName":pin["targetName"],"subjectExportsSha256":sha(&src)?,"compiler":pin["compilerCommit"],"target":pin["targetTriple"],"imports":rows}),
    )
}
