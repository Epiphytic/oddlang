use super::*;

fn fixture(files: &[(&str, &str)]) -> Result<tempfile::TempDir> {
    let temp = tempfile::tempdir()?;
    for (name, text) in files {
        let path = temp.path().join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, text)?;
    }
    exec(Command::new("cargo").current_dir(temp.path()).args([
        "+1.97.1",
        "generate-lockfile",
        "--offline",
    ]))?;
    Ok(temp)
}

fn options(root: &Path, package: &str, features: &[&str]) -> Result<Args> {
    let rustc = tool("rustc")?;
    let rustdoc = tool("rustdoc")?;
    Ok(Args {
        workspace: root.to_owned(),
        package: package.into(),
        kind: "lib".into(),
        target: package.replace('-', "_"),
        triple: txt(&tool_identity(&rustc, &rustdoc)?["host"])?.into(),
        no_default_features: false,
        features: strings(features),
        public_inherent: false,
        revision: None,
        previous_spans: None,
        out: root.join("report.json"),
    })
}

#[test]
fn missing_or_ambiguous_producer_is_rejected() {
    let invocation = "  Running `CARGO_MANIFEST_DIR=/src/reading /tool/rustc --crate-name reading --crate-type lib --out-dir /build -C extra-filename=-abc`\n";
    let packages = [(
        PathBuf::from("/src/reading"),
        json!({"name":"reading","version":"1.0","path":"reading"}),
    )]
    .into();
    for (log, count) in [("".to_owned(), 0), (invocation.repeat(2), 2)] {
        let error = profile::bindings(
            &log,
            "/tool/rustc",
            &strings(&["--extern", "value=/build/libreading-abc.rmeta"]),
            &packages,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(&format!("found {count}")),
            "{error}"
        );
    }
}

#[test]
fn unselected_host_producer_needs_no_target_package() -> Result<()> {
    let log = "  Running `CARGO_MANIFEST_DIR=/src/hostonly /tool/rustc --crate-name hostonly --crate-type dylib --out-dir /host`\n";
    assert_eq!(
        profile::bindings(log, "/tool/rustc", &[], &BTreeMap::new())?,
        json!({})
    );
    let e = profile::bindings(
        log,
        "/tool/rustc",
        &strings(&["--extern", "alias=/host/libhostonly.dylib"]),
        &BTreeMap::new(),
    )
    .unwrap_err();
    assert!(
        e.to_string()
            .contains("unknown Cargo package producing extern")
    );
    Ok(())
}

#[test]
fn cross_target_with_host_only_build_dependency() -> Result<()> {
    let target = "wasm32-wasip1";
    let installed = stdout(Command::new("rustup").args([
        "target",
        "list",
        "--installed",
        "--toolchain",
        TOOLCHAIN,
    ]))?;
    if !installed.lines().any(|t| t == target) {
        eprintln!("skipping: missing {target}");
        return Ok(());
    }
    let temp = fixture(&[
        (
            "Cargo.toml",
            "[package]\nname=\"cross-observer\"\nversion=\"1.0.0\"\nedition=\"2024\"\n[target.'cfg(not(target_family=\"wasm\"))'.build-dependencies]\nhostonly={path=\"hostonly\"}\n",
        ),
        (
            "hostonly/Cargo.toml",
            "[package]\nname=\"hostonly\"\nversion=\"1.0.0\"\nedition=\"2024\"\n",
        ),
        ("hostonly/src/lib.rs", "pub fn metadata() {}\n"),
        ("build.rs", "fn main() { hostonly::metadata(); }\n"),
        ("src/lib.rs", "pub struct Reading;\n"),
    ])?;
    let mut args = options(temp.path(), "cross-observer", &[])?;
    args.triple = target.into();
    let r = collect(&args)?.report;
    assert_eq!(r["profile"]["externBindings"], json!({}));
    assert!(
        arr(&r["comparison"]["matched"])?
            .iter()
            .any(|row| row["path"] == json!(["cross_observer", "Reading"]))
    );
    Ok(())
}

const ACCESS: &str = r#"
fn local() {}
mod outer {
    pub mod inner {
        pub struct Sample { hidden: u8, pub(super) limit: u8, pub(crate) count: u8, pub value: u8 }
        pub(in crate::outer) fn observe() {}
        pub struct Pos(pub(super) u8, pub u8);
        pub enum State { Ready }
    }
}
"#;

#[test]
fn declared_access_detects_restricted_visibility_changes() -> Result<()> {
    let temp = fixture(&[
        (
            "Cargo.toml",
            "[package]\nname=\"access-sample\"\nversion=\"1.0.0\"\nedition=\"2024\"\n",
        ),
        ("src/lib.rs", ACCESS),
    ])?;
    let args = options(temp.path(), "access-sample", &[])?;
    let before = collect(&args)?;
    let rows = &before.report["comparison"]["declaredAccess"];
    assert_eq!(rows["different"], json!([]));
    assert_eq!(rows["unresolved"], json!([]));
    let access: BTreeMap<_, _> = arr(&rows["agreement"])?
        .iter()
        .map(|r| {
            (
                r["path"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()
                    .as_str()
                    .unwrap(),
                &r["compiler"],
            )
        })
        .collect();
    assert_eq!(*access["local"], json!({"within":["access_sample"]}));
    assert_eq!(
        *access["hidden"],
        json!({"within":["access_sample","outer","inner"]})
    );
    assert_eq!(
        *access["limit"],
        json!({"within":["access_sample","outer"]})
    );
    assert_eq!(access["observe"], access["limit"]);
    assert_eq!(access["count"], access["local"]);
    assert_eq!(access["value"], "public");
    assert!(!access.contains_key("Ready"));
    for (i, expected) in [("0", access["limit"]), ("1", access["value"])] {
        let r = arr(&rows["agreement"])?
            .iter()
            .find(|v| v["path"] == json!(["access_sample", "outer", "inner", "Pos", i]))
            .unwrap();
        assert_eq!(&r["compiler"], expected);
    }
    let mut missing = before.compiler.clone();
    let field = arr(&missing["declarations"])?
        .iter()
        .find(|r| r["name"] == "limit")
        .unwrap()["id"]
        .clone();
    missing["edges"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["from"] != field || e["relation"] != "visibility-scope");
    let unknown=comparison::compare(&missing,&before.document,temp.path())?["declaredAccess"]["unresolved"].clone();
    assert_eq!(arr(&unknown)?.len(), 1);
    assert_eq!(
        unknown[0]["path"].as_array().unwrap().last().unwrap(),
        "limit"
    );
    fs::write(
        temp.path().join("src/lib.rs"),
        ACCESS.replace("pub(super) limit", "pub(crate) limit"),
    )?;
    let after = collect(&args)?;
    assert_eq!(
        after.report["comparison"]["declaredAccess"]["different"],
        json!([])
    );
    assert_eq!(
        after.report["comparison"]["declaredAccess"]["unresolved"],
        json!([])
    );
    let changed = comparison::compare(&before.compiler, &after.document, temp.path())?;
    let diffs = arr(&changed["declaredAccess"]["different"])?;
    assert_eq!(diffs.len(), 1);
    assert_eq!(
        diffs[0]["path"].as_array().unwrap().last().unwrap(),
        "limit"
    );
    assert_eq!(&diffs[0]["compiler"], access["limit"]);
    assert_eq!(&diffs[0]["rustdoc"], access["local"]);
    assert!(
        arr(&changed["matched"])?
            .iter()
            .any(|r| r["path"] == diffs[0]["path"])
    );
    Ok(())
}

const MANIFEST: &str = r#"[package]
name="specimen"
version="1.0.0"
edition="2024"
[features]
choose=["dep:first_read"]
[profile.release]
debug-assertions=true
[dependencies]
serde_derive="=1.0.229"
first_read={package="reading",path="old",optional=true}
next_read={package="reading",path="new"}
"#;
const SOURCE: &str = r#"
mod first { fn read() {} }
mod second { fn read() {} }
pub struct Sample { pub tag: u8 }
#[cfg(feature="choose")] pub type Prior = first_read::Reading;
pub type Next = next_read::Reading;
#[cfg(feature="choose")] pub fn selected() {}
#[cfg(debug_assertions)] pub fn asserts_enabled() {}
#[cfg(doc)] pub fn only_docs() {}
#[cfg(not(doc))] pub fn only_compiler() {}
"#;
fn names(report: &Value, group: &str) -> BTreeSet<Vec<String>> {
    report[group]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| serde_json::from_value(r["path"].clone()).unwrap())
        .collect()
}

#[test]
fn mutation_detects_changed_declarations_and_dependency_bindings() -> Result<()> {
    let temp = fixture(&[
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", SOURCE),
        (
            "old/Cargo.toml",
            "[package]\nname=\"reading\"\nversion=\"1.0.0\"\nedition=\"2024\"\n",
        ),
        ("old/src/lib.rs", "pub struct Reading;\n"),
        (
            "new/Cargo.toml",
            "[package]\nname=\"reading\"\nversion=\"2.0.0\"\nedition=\"2024\"\n[lib]\ncrate-type=[\"dylib\"]\n",
        ),
        ("new/src/lib.rs", "pub struct Reading;\n"),
    ])?;
    let args = options(temp.path(), "specimen", &["choose"])?;
    let before = collect(&args)?;
    let p = &before.report["profile"];
    let b = &p["externBindings"];
    assert_eq!(
        b["first_read"]["package"],
        json!({"name":"reading","version":"1.0.0","path":"old"})
    );
    assert_eq!(
        b["next_read"]["package"],
        json!({"name":"reading","version":"2.0.0","path":"new"})
    );
    assert_eq!(b["next_read"]["crateTypes"], json!(["dylib"]));
    assert_eq!(b["serde_derive"]["crateTypes"], json!(["proc-macro"]));
    assert_eq!(b["serde_derive"]["target"], json!([]));
    assert_eq!(b["serde_derive"]["package"]["version"], "1.0.229");
    assert!(txt(&b["serde_derive"]["package"]["source"])?.starts_with("registry+"));
    let comparison = &before.report["comparison"];
    let matched = names(comparison, "matched");
    for path in [
        &["specimen", "first", "read"][..],
        &["specimen", "second", "read"],
        &["specimen", "Sample", "tag"],
        &["specimen", "selected"],
        &["specimen", "asserts_enabled"],
    ] {
        assert!(matched.contains(&strings(path)));
    }
    assert_eq!(
        names(comparison, "compilerOnly"),
        [strings(&["specimen", "only_compiler"])].into()
    );
    assert_eq!(
        names(comparison, "rustdocOnly"),
        [strings(&["specimen", "only_docs"])].into()
    );
    let mut wrong = p.clone();
    wrong["explicitCfg"]
        .as_array_mut()
        .unwrap()
        .push(json!("feature=\"wrong\""));
    assert!(
        profile::compare_profiles(p, &wrong)
            .unwrap_err()
            .to_string()
            .contains("explicitCfg")
    );
    fs::write(
        temp.path().join("src/lib.rs"),
        SOURCE.replace("pub tag", "pub renamed"),
    )?;
    fs::write(
        temp.path().join("Cargo.toml"),
        MANIFEST
            .replace("path=\"old\",optional=true", "path=\"new\",optional=true")
            .replace(
                "next_read={package=\"reading\",path=\"new\"",
                "next_read={package=\"reading\",path=\"old\"",
            ),
    )?;
    let after = collect(&args)?;
    let q = &after.report["profile"];
    assert_eq!(q["externAliases"], p["externAliases"]);
    assert_eq!(q["externBindings"]["first_read"], b["next_read"]);
    assert!(
        profile::compare_profiles(p, q)
            .unwrap_err()
            .to_string()
            .contains("externBindings")
    );
    let new = names(&after.report["comparison"], "matched");
    let old = strings(&["specimen", "Sample", "tag"]);
    let renamed = strings(&["specimen", "Sample", "renamed"]);
    assert_eq!(matched.difference(&new).collect::<Vec<_>>(), [&old]);
    assert_eq!(new.difference(&matched).collect::<Vec<_>>(), [&renamed]);
    let mixed = comparison::compare(&before.compiler, &after.document, temp.path())?;
    assert!(names(&mixed, "compilerOnly").contains(&old));
    assert!(names(&mixed, "rustdocOnly").contains(&renamed));
    Ok(())
}
