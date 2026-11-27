use super::*;
use tempfile::tempdir;
#[test]
fn depinfo_preserves_escaped_paths_and_environment() {
    let (paths, env) = inputs::dep_info(
        "output.d: src/main.rs src/a\\ b.rs \\\n src/c.rs\n# env-dep:ABSENT\n# env-dep:VALUE=x=y\n",
    )
    .unwrap();
    assert_eq!(paths, ["src/main.rs", "src/a b.rs", "src/c.rs"]);
    assert_eq!(json!(env), json!({"ABSENT":null,"VALUE":"x=y"}));
}
#[test]
fn later_items_are_outside_the_scoped_test_module() {
    let module = b"#[cfg(test)]\nmod tests {\n    fn first() {}\n}";
    let mut extended = module.to_vec();
    extended.extend(b"\nfn following() {\n}\n");
    let a = span(module, "fixture.rs").unwrap().unwrap();
    assert_eq!(a, span(&extended, "fixture.rs").unwrap().unwrap());
    assert_eq!(a["endByte"], module.len());
}
#[test]
fn rejects_an_unreferenced_extra_package_source() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let dir = root.join(PACKAGE_ROOT).join("src");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lib.rs"), "fn f() {}\n").unwrap();
    fs::write(dir.join("unexpected.rs"), "").unwrap();
    let old = json!({"crates/marmish-stream/src/lib.rs":"0".repeat(40)});
    let error = tracked(&root, object(&old).unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("extra") && error.contains("unexpected.rs"));
}
#[test]
fn rejects_orphan_compiler_depinfo() {
    let temp = tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let src = root.join("src");
    let target = root.join("target");
    let deps = target.join("debug/deps");
    fs::create_dir(&src).unwrap();
    fs::create_dir_all(&deps).unwrap();
    fs::write(src.join("foo.rs"), "").unwrap();
    for n in ["foo", "orphan"] {
        fs::write(deps.join(format!("{n}.d")), "foo.d: foo.rs\n").unwrap();
    }
    let compiler = root.join("compiler");
    let cmd = format!(
        " Running `{}/bin/rustc --crate-name foo foo.rs --out-dir {}`",
        compiler.display(),
        deps.display()
    );
    let declared = json!({"foo.rs":"tracked"});
    let paths = Roots {
        source: &src,
        target: &target,
        home: &root.join("home"),
        vendor: None,
        tools: &compiler,
        sdk: &root.join("sdk"),
        declared: object(&declared).unwrap(),
    };
    let error = paths
        .inventory(&commands(&cmd).unwrap(), &BTreeMap::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("unpaired rustc dep-info") && error.contains("orphan.d"));
}
#[test]
fn real_rustc_include_bytes_must_be_declared() {
    let temp = tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let src = root.join("lib.rs");
    let output = root.join("lib.d");
    fs::write(
        &src,
        "pub const BYTES: &[u8] = include_bytes!(\"undeclared.txt\");\n",
    )
    .unwrap();
    fs::write(root.join("undeclared.txt"), "compiler input canary").unwrap();
    assert!(
        Command::new("rustc")
            .arg("+1.97.1")
            .args(["--crate-type", "lib", "--emit"])
            .arg(format!("dep-info={}", output.display()))
            .arg(&src)
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let (raw, _) = inputs::dep_info(&fs::read_to_string(output).unwrap()).unwrap();
    let unexpected = raw.iter().find(|p| p.ends_with("/undeclared.txt")).unwrap();
    let old = json!({"lib.rs":"tracked"});
    let paths = Roots {
        source: &root,
        target: &root.join("target"),
        home: &root.join("home"),
        vendor: None,
        tools: &root.join("compiler"),
        sdk: &root.join("sdk"),
        declared: object(&old).unwrap(),
    };
    assert!(
        paths
            .observed(Path::new(unexpected), &BTreeMap::new())
            .unwrap_err()
            .to_string()
            .contains("undeclared compiler input undeclared.txt")
    );
}
#[test]
fn rejects_same_workspace_or_changed_frozen_inputs() {
    let mut original = json!({"role":"original","workspaceRoot":"upstream","builds":{}});
    for k in [
        "rustc",
        "cargo",
        "target",
        "targetCfg",
        "sourceRemapping",
        "cargoInputs",
        "trackedSources",
        "inlineTestSpans",
    ] {
        original[k] = json!("frozen");
    }
    let mut candidate = original.clone();
    candidate["role"] = json!("candidate");
    assert!(
        compare(&original, &candidate)
            .unwrap_err()
            .to_string()
            .contains("candidate is the original workspace")
    );
    candidate["workspaceRoot"] = json!("candidate");
    compare(&original, &candidate).unwrap();
    candidate["trackedSources"] = json!("changed");
    assert!(
        compare(&original, &candidate)
            .unwrap_err()
            .to_string()
            .contains("baseline mismatch: trackedSources")
    );
}
