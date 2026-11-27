use super::*;
use syntax::Syntax;
#[test]
fn file_classification_is_closed() {
    for (path, class) in [
        ("scripts/install.sh", "generated-artifact"),
        ("crates/marmish-native/build.rs", "build-input"),
        ("tests/fixtures/relay-canonicalization.json", "test-fixture"),
    ] {
        assert_eq!(source::classify_file(path).unwrap(), class);
    }
    assert!(source::classify_file("unexpected/runtime.xyz").is_err());
}
#[test]
fn scope_ordinals_are_local_and_one_based() {
    let ids = BTreeMap::from([
        ("a.rs".into(), vec!["a/f".into(), "a/g".into()]),
        ("b.rs".into(), vec!["b/f".into()]),
    ]);
    assert_eq!(
        source::scope("b.rs", &json!([0, "use", "T", 1]), &ids).unwrap(),
        None
    );
    assert_eq!(
        source::scope("b.rs", &json!([1, "call", "f", 2]), &ids).unwrap(),
        Some("b/f")
    );
    assert!(source::scope("b.rs", &json!([2, "call", "g", 2]), &ids).is_err());
}
#[test]
fn open_reachability_rejects_completeness() {
    assert!(
        require_complete(&json!(["reachable callees remain unresolved"]))
            .unwrap_err()
            .to_string()
            .contains("reachable")
    );
}
#[test]
fn cfg_helpers_and_platform_modules_follow_cargo_root() {
    let targets =
        json!({"c:lib:c":{"kind":"lib","name":"c","path":"src/lib.rs","requiredFeatures":[]}});
    let mut files: Map = ["src/lib.rs", "src/system.rs", "src/platform/apple.rs"]
        .into_iter()
        .map(|p| (p.into(), json!({})))
        .collect();
    let syntax = BTreeMap::from([
        (
            "src/lib.rs".into(),
            Syntax {
                edges: vec![json!({"kind":"mod","path":null,"name":"system","inline":[],"line":4})],
                attributes: vec![(4, 4, vec!["#[cfg(test)]".into()])],
                ..Syntax::default()
            },
        ),
        (
            "src/system.rs".into(),
            Syntax {
                edges: vec![
                    json!({"kind":"mod","path":"platform/apple.rs","name":"platform","inline":[],"line":2}),
                ],
                attributes: vec![(2, 2, vec!["#[cfg(target_os = \"macos\")]".into()])],
                ..Syntax::default()
            },
        ),
    ]);
    let (ctx, gaps) = source::contexts(&files, &targets, &syntax).unwrap();
    assert!(gaps.is_empty());
    let rows = &ctx["src/platform/apple.rs"];
    assert_eq!(
        rows[0]["attributes"],
        json!(["#[cfg(target_os = \"macos\")]", "#[cfg(test)]"])
    );
    assert_eq!(
        source::classification(
            "crates/c/src/platform/apple.rs",
            "cargo-source",
            rows,
            &[],
            &targets
        )
        .unwrap()
        .0,
        "test"
    );
    files.remove("src/platform/apple.rs");
    assert_eq!(
        source::contexts(&files, &targets, &syntax).unwrap().1,
        BTreeSet::from([("src/system.rs".into(), 2, "c:lib:c".into())])
    );
}
#[test]
fn predicates_distinguish_unknown_gates() {
    let attrs = vec![
        "#[cfg(all(test,target_os=\"linux\"))]".into(),
        "#[ignore = \"host gate\"]".into(),
    ];
    for (platform, active) in [("linux", true), ("macos", false)] {
        assert_eq!(
            platforms::eligible(
                &attrs,
                &Set::from(["test".into(), format!("target_os=\"{platform}\"")])
            ),
            Some(active)
        );
    }
    assert_eq!(
        platforms::eligible(
            &["#[cfg(undocumented_build_script_cfg)".into()],
            &Set::from(["test".into()])
        ),
        None
    );
    assert_eq!(
        platforms::eligible(
            &["#[cfg_attr(test, track_caller)]".into()],
            &Set::from(["test".into()])
        ),
        Some(true)
    );
}
#[test]
fn union_and_target_units_are_distinct() {
    let sites = [
        json!({"id":"A","kind":"function_item","classification":"retained-native","targets":["a","b"]}),
    ];
    let files = object(&json!({"crate.rs":{"parseStatus":"recovered"}}))
        .unwrap()
        .clone();
    let d = platforms::denominators(&sites, &files, &["host".into()], &["default".into()]).unwrap();
    assert_eq!(d["lexicalUnion"]["observedSites"], 1);
    assert_eq!(d["lexicalUnion"]["byCargoTarget"]["a"]["function_item"], 1);
    assert!(d["compiledRuntimeUnits"]["host"]["default"]["count"].is_null());
}
#[test]
fn frozen_ignored_and_packaging_evidence_follows_native_matrix() {
    let summary: Value = serde_json::from_str(include_str!(
        "../../../examples/marmish/inventory-summary.json"
    ))
    .unwrap();
    for (target, p) in object(&summary["platformInventory"]).unwrap() {
        let apple = target == "aarch64-apple-darwin";
        let ignored = strings(&p["ignoredSites"]).unwrap();
        assert_eq!(ignored.iter().any(|i| i.contains("native_macos.rs")), apple);
        assert_eq!(
            ignored.iter().any(|i| i.contains("native_linux.rs")),
            !apple
        );
        let package = &p["packaging"];
        let recipes = array(&package["recipes"]).unwrap();
        assert_eq!(recipes.len(), 2);
        for r in recipes {
            assert_eq!(r["target"], *target);
            assert_eq!(r["signing"], if apple { "adhoc" } else { "none" });
            assert_eq!(
                r["workflowSha256"],
                package["trackedRecipeInputs"][r["workflow"].as_str().unwrap()]
            );
        }
        let inputs = array(&package["finalInputs"]).unwrap();
        assert!(inputs.iter().all(|f| f["sha256"].is_null()));
        let paths: BTreeSet<_> = inputs.iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert_eq!(
            paths,
            BTreeSet::from([
                "package-input/marmish",
                "package-input/marmishd",
                "package-input/marmish-worker",
                "THIRD-PARTY-LICENSES"
            ])
        );
    }
}
#[test]
fn platform_tests_preserve_ignored_and_unknown_selections() {
    let path = "crates/fixture/src/lib.rs";
    let target = "fixture:lib:fixture";
    let mut src = Syntax::default();
    for (line, name, attr) in [
        (2, "apple", "#[cfg(target_os = \"macos\")]"),
        (4, "linux", "#[cfg(target_os = \"linux\")]"),
        (6, "custom", "#[cfg(compiler_supplied_switch)]"),
        (8, "manual", "#[ignore = \"explicit gate\"]"),
    ] {
        src.tests
            .push(json!({"line":line,"name":name,"attributes":["#[test]",attr]}));
        src.attributes.push((line, line, vec![attr.into()]));
    }
    let files = object(&json!({path:{"cargoContexts":[{"target":target,"attributes":[]}]}}))
        .unwrap()
        .clone();
    let cfgs = BTreeMap::from([
        (
            "aarch64-apple-darwin".into(),
            Set::from(["target_os=\"macos\"".into()]),
        ),
        (
            "x86_64-unknown-linux-gnu".into(),
            Set::from(["target_os=\"linux\"".into()]),
        ),
    ]);
    let (catalog, selected) = platforms::tests(
        &BTreeMap::from([(path.into(), src)]),
        &[],
        &files,
        &json!({target:{"crate":"fixture"}}),
        &cfgs,
        &["default".into()],
    )
    .unwrap();
    for (p, local) in [
        ("aarch64-apple-darwin", "apple"),
        ("x86_64-unknown-linux-gnu", "linux"),
    ] {
        let mut rows = Map::new();
        for r in &selected[p] {
            assert_eq!(r["cargoTarget"], target);
            let site = string(&r["test"]).unwrap();
            rows.insert(
                string(&catalog[site]["name"]).unwrap().into(),
                json!([r["gate"], r["profiles"], r["uncertainProfiles"]]),
            );
        }
        assert_eq!(
            json!(rows),
            json!({local:["ordinary",["default"],[]],
                "custom": ["ordinary",[],["default"]],
                "manual": ["ignored",["default"],[]]
            })
        );
    }
}
