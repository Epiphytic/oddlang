//! Compare actual adapter edges with compiler MIR and independent public tests.
use super::tests::{fixtures, host};
use super::*;
fn mir(root: &Path) -> Result<BTreeSet<String>> {
    checked(cmd("cargo").current_dir(root).args([
        "rustc",
        "--locked",
        "--offline",
        "--lib",
        "--features",
        "extra",
        "--",
        "--emit=mir",
    ]))?;
    let files = fs::read_dir(root.join("target/debug/deps"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|v| v == "mir")
                && p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("sample-")
        })
        .collect::<Vec<_>>();
    let [file] = files.as_slice() else {
        return Err("expected one sample MIR file".into());
    };
    let text = fs::read_to_string(file)?;
    let (_, tail) = text
        .split_once("\nfn private(_1: &mut Item)")
        .ok_or("missing private MIR")?;
    let (body, _) = tail.split_once("\n}").ok_or("unterminated MIR body")?;
    let mut calls = BTreeSet::new();
    for line in body.lines() {
        if line.contains(" -> [")
            && line.contains("return:")
            && let Some((_, call)) = line.split_once("= ")
            && let Some((name, _)) = call.split_once('(')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            calls.insert(name.to_owned());
        }
    }
    Ok(calls)
}
pub fn verify(root: &Path) -> Result {
    let base = fs::read_to_string(fixtures().join("adapter.rs"))?;
    fs::create_dir_all(root.join("actual/src"))?;
    fs::create_dir(root.join("src"))?;
    fs::create_dir(root.join("tests"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='sample'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nrenamed={path='actual',package='actual'}\n[features]\nextra=[]\n",
    )?;
    fs::write(
        root.join("actual/Cargo.toml"),
        "[package]\nname='actual'\nversion='0.1.0'\nedition='2024'\n",
    )?;
    fs::write(
        root.join("actual/src/lib.rs"),
        "pub fn combine(a:u8)->u8{a}",
    )?;
    fs::write(root.join("src/lib.rs"), &base)?;
    fs::write(
        root.join("tests/public.rs"),
        "#[test] fn unchanged_consumer(){assert_eq!(sample::alias(),1);assert_eq!(sample::concrete(),3);}",
    )?;
    checked(
        cmd("cargo")
            .current_dir(root)
            .args(["generate-lockfile", "--offline"]),
    )?;
    let host = host()?;
    let inventory = |source: &str, extra: bool, polluted: bool| -> Result<Value> {
        fs::write(root.join("src/lib.rs"), source)?;
        let mut c = provider_validation::capture_command();
        c.arg(root.join("Cargo.toml"))
            .args(["sample", "lib", "sample", &host, "no-default"]);
        if extra {
            c.arg("extra");
        }
        if polluted {
            c.env("RUSTFLAGS", "--cfg unpinned_rustflag")
                .env("CARGO_ENCODED_RUSTFLAGS", "--cfg\u{1f}unpinned_encoded")
                .env("CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS", "true");
        }
        Ok(serde_json::from_slice(&checked(&mut c)?)?)
    };
    let tests = || {
        checked(cmd("cargo").current_dir(root).args([
            "test",
            "--locked",
            "--offline",
            "--test",
            "public",
        ]))
    };
    let report = inventory(&base, true, false)?;
    tests()?;
    assert_eq!(
        mir(root)?,
        ["first", "combine", "generated"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    let decl = array(&report["declarations"])?;
    let private = "local::sample::private";
    let names = |g: &Value| -> Result<BTreeSet<String>> {
        array(&g["declarations"])?
            .iter()
            .map(|r| Ok(string(&r["id"])?.to_owned()))
            .collect()
    };
    let lookup = |name: &str| decl.iter().find(|r| r["name"] == name).unwrap();
    let row = decl.iter().find(|r| r["id"] == private).unwrap();
    let ids = decl
        .iter()
        .filter(|r| r["name"] == "private")
        .map(|r| string(&r["id"]))
        .collect::<Result<BTreeSet<_>>>()?;
    assert_eq!(
        ids,
        BTreeSet::from([private, "local::sample::helpers::{type}::private"])
    );
    let formatted = inventory(
        &base.replace("fn private(value", "\n\nfn private(value"),
        true,
        false,
    )?;
    assert_eq!(names(&report)?, names(&formatted)?);
    assert_eq!(report["edges"], formatted["edges"]);
    assert_eq!(report["gaps"], formatted["gaps"]);
    assert_eq!(row["visibility"], "private");
    assert_eq!(lookup("generated")["macroGenerated"], true);
    assert_eq!(
        row["signature"]["output"],
        json!({"kind":"primitive","name":"u8"})
    );
    let has = |g: &Value, from: &str, rel: &str, to: &str| {
        g["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["from"] == from && r["relation"] == rel && r["to"] == to)
    };
    assert!(has(
        &report,
        string(&lookup("alias")["id"])?,
        "reexport",
        string(&lookup("first")["id"])?
    ));
    let first = string(&lookup("first")["id"])?;
    let second = string(&lookup("second")["id"])?;
    let part = string(&lookup("part")["id"])?;
    assert!(has(&report, private, "call", first));
    assert!(!has(&report, private, "call", second));
    assert!(has(&report, private, "call", "external::actual::combine"));
    assert!(has(&report, private, "read", part));
    assert!(has(&report, private, "write", part));
    let a = "local::sample::helpers::{type}::a";
    let b = "local::sample::helpers::{type}::b";
    let pick = "local::sample::pick";
    assert!(has(&report, pick, "function-reference", a));
    for (owner, kind) in [
        ("local::sample::scope", "implicit-drop"),
        ("local::sample::nested", "nested-field-projection"),
        ("local::sample::inline_const", "skipped-body"),
        (
            "local::sample::Discriminant::{type}::Variant::{type}",
            "skipped-body",
        ),
        (string(&lookup("dynamic")["id"])?, "dynamic-dispatch"),
    ] {
        assert!(array(&report["gaps"])?.contains(&json!({"owner":owner,"kind":kind})));
    }
    assert!(!decl.iter().any(|r| r["name"] == "extra_unpinned"));
    assert_eq!(report, inventory(&base, true, true)?);
    let changed = base
        .replace("helpers::first() +", "helpers::second() +")
        .replace("fn unused_private() -> u8 { 3 }", "")
        .replace("map(helpers::a)", "map(helpers::b)");
    let other = inventory(&changed, false, false)?;
    tests()?;
    assert_eq!(
        mir(root)?,
        ["second", "combine", "generated"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    let removed = names(&report)?
        .difference(&names(&other)?)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        removed,
        [
            "local::sample::dynamic",
            "local::sample::dynamic::'_::{lifetime}",
            "local::sample::unused_private"
        ]
        .map(String::from)
        .into_iter()
        .collect()
    );
    assert!(!has(&other, private, "call", first));
    assert!(has(&other, private, "call", second));
    assert!(!has(&other, pick, "function-reference", a));
    assert!(has(&other, pick, "function-reference", b));
    Ok(())
}
