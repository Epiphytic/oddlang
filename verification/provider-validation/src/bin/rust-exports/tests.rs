//! Native caller fixtures for the export and resolution probes.
use super::*;
pub(super) fn fixtures() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/rust-recovery"
    ))
}
pub(super) fn host() -> Result<String> {
    Ok(version()?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("missing host")?
        .to_owned())
}
pub(super) fn crate_files(root: &Path, manifest: &str, source: &str) -> Result {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::write(root.join("src/lib.rs"), source)?;
    checked(
        cmd("cargo")
            .current_dir(root)
            .args(["generate-lockfile", "--offline"]),
    )?;
    Ok(())
}
pub(super) fn checked_output(command: &mut Command, expect: Option<&str>) -> Result<Output> {
    let out = capture(command)?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    match expect {
        None if out.status.success() => (),
        Some(e) if !out.status.success() && stderr.contains(e) => (),
        _ => return Err(format!("{command:?}: expected {expect:?}: {stderr}").into()),
    }
    Ok(out)
}
pub fn run(name: &str, cli: &Path) -> Result {
    let cli = cli.canonicalize()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    match name {
        "shapes" => super::shapes::verify(&cli, root),
        "inherent" => super::semantics::inherent(&cli, root),
        "public-api" => super::semantics::api(&cli, root),
        "value-domain" => super::semantics::value(&cli, root),
        "shared-source" => super::semantics::shared(&cli, root),
        "exports" => exports(&cli, root),
        "consumer-paths" => consumers(&cli, root),
        _ => return Err(format!("unknown Rust export fixture: {name}").into()),
    }
}
fn consumers(cli: &Path, root: &Path) -> Result {
    let mut packages = Vec::new();
    for name in ["old", "new"] {
        let p = root.join(name);
        let m = p.join("Cargo.toml");
        crate_files(
            &p,
            "[package]\nname='fixture'\nversion='1.0.0'\nedition='2024'\n",
            &fs::read_to_string(fixtures().join(format!("consumer-{name}.rs")))?,
        )?;
        fs::create_dir(p.join(".cargo"))?;
        fs::write(
            p.join(".cargo/config.toml"),
            "[build]\nrustflags=['--cfg','local_profile']\nrustdocflags=['--cfg','local_profile']\n",
        )?;
        let out = root.join(format!("{name}-observed"));
        observer::probe(&[
            cli.display().to_string(),
            m.display().to_string(),
            "fixture".into(),
            out.display().to_string(),
            "--default-features".into(),
        ])?;
        packages.push((out, m));
    }
    let result = |a: usize, b: usize| {
        downstream::probe(
            &packages[a].0.join("recovered/exports.json"),
            &packages[b].0.join("recovered"),
            &packages[b].1,
            &packages[b].0.join("native"),
        )
    };
    let r = result(0, 1)?;
    assert_eq!(r["expectedUnresolved"], json!([["_"]]));
    let rows: BTreeMap<Spelling, &Value> = array(&r["imports"])?
        .iter()
        .map(|r| Ok((paths(&r["spelling"])?, r)))
        .collect::<Result<_>>()?;
    for n in ["LocalType", "Held"] {
        let row = rows[&vec![n.into()]];
        assert_eq!(row["importCompiles"], true);
        assert_eq!(row["subjectKind"], "data");
    }
    for (n, e) in [
        ("Gone", "E0432"),
        ("Hidden", "E0603"),
        ("Clash", "ambiguous_glob_imports"),
    ] {
        let row = rows[&vec![n.into()]];
        assert_eq!(row["errorCodes"], json!([e]));
        assert_eq!(row["importCompiles"], false);
    }
    for n in ["Meter", "Mac"] {
        let row = rows[&vec![n.into()]];
        assert_eq!(row["importCompiles"], true);
        assert!(row["subjectKind"].is_null());
    }
    assert!(rows[&vec!["Clash".into()]]["subjectAmbiguity"].is_object());
    assert_eq!(
        rows[&vec!["removed".into(), "Leaf".into()]]["importCompiles"],
        false
    );
    let inverse = result(1, 0)?;
    let clash = array(&inverse["imports"])?
        .iter()
        .find(|r| r["spelling"] == json!(["Clash"]))
        .unwrap();
    assert!(clash["expectedAmbiguity"].is_object());
    assert_eq!(clash["importCompiles"], true);
    assert_eq!(clash["subjectKind"], "data");
    fs::create_dir(root.join(".cargo"))?;
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustflags=['--cfg','unreviewed']\n",
    )?;
    let old = env::current_dir()?;
    env::set_current_dir(root)?;
    let error = result(0, 1).unwrap_err();
    env::set_current_dir(old)?;
    assert!(error.to_string().contains("configuration differs"));
    println!("Verified both consumer directions and Cargo configuration pin");
    Ok(())
}
fn exports(cli: &Path, root: &Path) -> Result {
    let source = fs::read_to_string(fixtures().join("export-lib.rs"))?;
    let pkg = root.join("domain");
    let lib = pkg.join("src/lib.rs");
    let manifest =
        "[package]\nname='observations'\nversion='0.1.0'\nedition='2024'\n[features]\nextra=[]\n";
    crate_files(&pkg, manifest, &source)?;
    let recover = |name: &str, p: &Path, features: &[String]| -> Result<Value> {
        let profile = root.join("profile.toml");
        fs::write(
            &profile,
            observer::profile(
                &p.join("Cargo.lock"),
                "observations",
                "observations",
                &host()?,
                features,
                false,
            )?,
        )?;
        let dest = root.join(name);
        checked(
            cmd(cli)
                .current_dir(root)
                .args(["recover", "rust-exports"])
                .arg(p.join("Cargo.toml"))
                .arg("--profile")
                .arg(profile)
                .arg("--out")
                .arg(&dest),
        )?;
        let graph = dest.join("exports.json");
        assert_eq!(
            read(&dest.join("exports.lock"))?["exportsSha256"],
            sha(&graph)?
        );
        read(&graph)
    };
    let caller = root.join("caller");
    fs::create_dir_all(caller.join("src"))?;
    let caller_manifest = "[package]\nname='consumer'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nobservations={path='../domain'}\n";
    fs::write(caller.join("Cargo.toml"), caller_manifest)?;
    let probe = |src: &str, error: Option<&str>| -> Result {
        fs::write(caller.join("src/lib.rs"), src)?;
        checked_output(
            cmd("cargo")
                .current_dir(&caller)
                .args(["check", "--offline", "--quiet"]),
            error,
        )?;
        Ok(())
    };
    let g = recover("baseline", &pkg, &[])?;
    let relocated = root.join("relocated-source");
    crate_files(&relocated, manifest, &source)?;
    assert_eq!(recover("relocated", &relocated, &[])?, g);
    for file in ["exports.json", "exports.lock"] {
        assert_eq!(
            fs::read(root.join("baseline").join(file))?,
            fs::read(root.join("relocated").join(file))?
        );
    }
    let independent = root.join("independent");
    observer::probe(&[
        cli.display().to_string(),
        pkg.join("Cargo.toml").display().to_string(),
        "observations".into(),
        independent.display().to_string(),
    ])?;
    let evidence = read(&independent.join("evidence.json"))?;
    assert!(evidence["compilePositiveCount"].as_u64().unwrap() > 10);
    assert!(evidence["fieldProbes"]["publicReads"].as_u64().unwrap() >= 5);
    let modules = &g["modules"];
    let top = &modules[string(&g["root"])?]["bindings"];
    let report = &top["Report"]["target"];
    for n in ["hidden", "Extra", "_"] {
        assert!(top.get(n).is_none());
    }
    for n in ["routes", "glob"] {
        let child = &modules[string(&top[n]["target"])?]["bindings"];
        assert_eq!(child["Alias"]["target"], *report);
        assert_eq!(child["again"]["target"], g["root"]);
    }
    let mixed = &modules[string(&top["mixed"]["target"])?];
    assert!(mixed["bindings"].get("Conflict").is_none());
    let conflict = &mixed["ambiguous"]["Conflict"];
    let mut choices = vec![&conflict["primary"]];
    choices.extend(array(&conflict["alternatives"])?);
    let mut targets = BTreeSet::new();
    for c in choices {
        assert_eq!(c["audience"], json!({"kind":"public"}));
        targets.insert(string(&c["binding"]["target"])?.replace("::{type}", ""));
    }
    assert_eq!(
        targets,
        BTreeSet::from([
            "local::observations::left::Conflict".into(),
            "local::observations::right::Conflict".into()
        ])
    );
    probe(
        "#![deny(ambiguous_glob_imports)]\npub type Invalid=observations::mixed::Conflict;",
        Some("ambiguous"),
    )?;
    let target = string(&conflict["primary"]["binding"]["target"])?.replace("::{type}", "");
    let picked = target.rsplit("::").nth(1).unwrap();
    probe(
        &format!(
            "#![allow(ambiguous_glob_imports)]\npub fn chosen(value:observations::mixed::Conflict)->u8{{value.from_{picked}}}"
        ),
        None,
    )?;
    assert_eq!(
        modules[string(&top["routes"]["target"])?]["path"],
        json!(["mirror"])
    );
    assert_eq!(top["ReadingAlias"]["kind"], "alias");
    assert_ne!(top["ReadingAlias"]["target"], *report);
    assert_eq!(top["External"]["kind"], "external");
    assert_eq!(top["Named"]["kind"], "variant");
    let fields = &g["data"][string(report)?]["fields"];
    assert_eq!(fields["units"]["audience"], json!({"kind":"public"}));
    assert_eq!(
        fields["calibration"]["audience"],
        json!({"kind":"scope","id":g["root"]})
    );
    for (src, error) in [
        (
            "pub fn inaccessible(r:observations::Report)->i8{r.calibration}",
            Some("E0616"),
        ),
        (
            "pub type Private=observations::hidden::Reading;",
            Some("E0603"),
        ),
        ("pub use observations::mirror::again as cycle_edge;", None),
        ("pub use observations::outward::nested as alias_edge;", None),
        (
            "pub fn named(v:observations::Payload){if let observations::Payload::Named{item}=v{let _=item;}}",
            None,
        ),
        (
            "pub fn tuple(v:observations::Payload){if let observations::Payload::Tuple(item)=v{let _=item;}}",
            None,
        ),
        (
            "pub fn bits(v:observations::Bits)->u32{unsafe{v.first}}",
            None,
        ),
        (
            "pub fn construct()->observations::Incomplete{observations::Incomplete{public_field:true}}",
            Some("non-exhaustive"),
        ),
        (
            "pub fn construct()->observations::Tuple{observations::Tuple(0,0)}",
            Some("E0423"),
        ),
        ("pub fn first(r:observations::Tuple)->i16{r.0}", None),
        ("pub fn second(r:observations::Tuple)->i8{r.1}", Some("")),
    ] {
        probe(src, error)?;
    }
    fs::write(&lib, source.replace("Report", "Summary"))?;
    let changed = recover("changed", &pkg, &["extra".into()])?;
    let t = &changed["modules"][string(&changed["root"])?]["bindings"];
    assert!(t.get("Report").is_none());
    assert_eq!(t["Summary"]["target"], *report);
    assert_eq!(t["Extra"]["target"], *report);
    probe("pub fn new(r:observations::Summary)->u16{r.units}", None)?;
    probe("pub type Old=observations::Report;", Some("E0425"))?;
    fs::write(
        caller.join("Cargo.toml"),
        caller_manifest.replace("path='../domain'", "path='../domain',features=['extra']"),
    )?;
    probe(
        "pub use observations::Extra as independently_selected;",
        None,
    )?;
    let vis = fs::read_to_string(fixtures().join("visibility.rs"))?;
    fs::write(&lib, &vis)?;
    checked_output(
        cmd("cargo")
            .current_dir(&pkg)
            .args(["check", "--locked", "--offline"]),
        Some("ambiguous_import_visibilities"),
    )?;
    fs::write(
        &lib,
        vis.replace(
            "deny(ambiguous_import_visibilities)",
            "warn(ambiguous_import_visibilities)",
        ),
    )?;
    let r = checked_output(
        cmd("cargo")
            .current_dir(&pkg)
            .args(["check", "--locked", "--offline"]),
        None,
    )?;
    assert!(String::from_utf8_lossy(&r.stderr).contains("ambiguous_import_visibilities"));
    let g = recover("visibility-lint-allowed", &pkg, &[])?;
    let m = &g["modules"];
    let museum = &m[string(&m[string(&g["root"])?]["bindings"]["museum"]["target"])?];
    assert!(museum["bindings"].get("Renamed").is_some());
    assert!(museum["ambiguous"].get("Renamed").is_none());
    probe(
        "pub use observations::museum::Renamed as chosen_visibility;",
        None,
    )?;
    println!(
        "Checked aliases, public module cycles, visibility and changed export names against native callers"
    );
    Ok(())
}
