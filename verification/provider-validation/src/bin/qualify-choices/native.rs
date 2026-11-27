use super::*;
const COMPASS: &str = fixture!("compass.rs");
const EDITION: &str = "--edition=2021";

pub fn verify(cli: &Path, root: &Path) -> Result {
    for (label, source, expected) in [
        (
            "baseline",
            COMPASS.to_owned(),
            ["(\"inland\", 0)", "(\"coast\", 1)", "(\"ridge\", 2)"],
        ),
        (
            "mutant",
            COMPASS.replace("Inland, Coast", "Coast, Inland"),
            ["(\"inland\", 1)", "(\"coast\", 0)", "(\"ridge\", 2)"],
        ),
    ] {
        selection(
            cli,
            root,
            &source,
            "urn:compass:v1",
            "Compass",
            "v1",
            label,
            None,
        )?;
        let product = format!("{label}-rust");
        project(
            cli,
            root,
            "rust",
            &format!("{label}/graph.json"),
            &format!("{label}/rust.json"),
            &product,
            None,
        )?;
        for path in ["choice.rs", &format!("{product}/values.rs")] {
            library(root, "compass", path, EDITION)?;
            caller(
                root,
                "compass",
                fixture!("compass-caller.rs"),
                EDITION,
                None,
            )?;
            let stdout = String::from_utf8(run(root, "./caller", &[], None)?)?;
            assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
        }
    }
    caller(
        root,
        "compass",
        r#"fn main(){println!("{:?}",compass::Compass::Inland);}"#,
        EDITION,
        Some("Debug"),
    )?;
    let invalid: Vec<String> = serde_json::from_str(fixture!("invalid-compass.json"))?;
    for (i, source) in invalid.iter().enumerate() {
        selection(
            cli,
            root,
            source,
            "urn:compass:v1",
            "Compass",
            "v1",
            &format!("invalid-{i}"),
            Some(""),
        )?;
    }
    let original = read(&root.join("baseline/rust.json"))?;
    let types = original["types"].as_object().unwrap();
    assert_eq!(types.len(), 1);
    let id = types.keys().next().unwrap().clone();
    let mut order = original["types"][&id]["order"].as_array().unwrap().clone();
    order.reverse();
    for (key, value) in [
        ("traits", json!(["copy", "hash"])),
        ("view", json!("as_label")),
        ("order", json!(order)),
    ] {
        let mut binding = original.clone();
        binding["types"][&id][key] = value;
        write(&root.join("invalid.json"), &binding)?;
        project(
            cli,
            root,
            "rust",
            "baseline/graph.json",
            "invalid.json",
            "invalid-overlay",
            Some("plain choice native binding"),
        )?;
    }
    let mut graph = read(&root.join("baseline/graph.json"))?;
    let mut binding = original;
    graph["types"]["urn:compass:wrapper"] =
        json!({"kind":"record","fields":{"urn:compass:wrapper:field":{"kind":"named","id":id}}});
    binding["types"]["urn:compass:wrapper"] = json!({"name":"Wrapper","order":["urn:compass:wrapper:field"],"shape":"named","members":{"urn:compass:wrapper:field":{"name":"value","public":true}}});
    write(&root.join("invalid.json"), &binding)?;
    write(&root.join("graph.json"), &graph)?;
    project(
        cli,
        root,
        "rust",
        "graph.json",
        "invalid.json",
        "wrapper",
        Some("field lacks Debug"),
    )?;
    println!(
        "Verified source order, native trait bounds and {} rejected enum sources",
        invalid.len()
    );
    Ok(())
}

pub fn capabilities(cli: &Path, root: &Path) -> Result {
    for (label, source, copy, expected) in [
        ("baseline", fixture!("bearing.rs"), false, "East=0 West=1\n"),
        (
            "mutation",
            fixture!("bearing-copy.rs"),
            true,
            "East=1 West=0\n",
        ),
    ] {
        let dir = root.join(label);
        fs::create_dir(&dir)?;
        selection(
            cli,
            &dir,
            source,
            "urn:fixture:route:choice",
            "Bearing",
            "v2",
            "recovered",
            None,
        )?;
        let mut graph = read(&dir.join("recovered/graph.json"))?;
        let types = graph["types"].as_object_mut().unwrap();
        assert_eq!(types.len(), 1);
        let def = types.values_mut().next().unwrap();
        assert_eq!(def["copy"].as_bool().unwrap_or(true), copy);
        assert_eq!(def["debug"], true);
        def["copy"] = json!(!copy);
        project(
            cli,
            &dir,
            "rust",
            "recovered/graph.json",
            "recovered/rust.json",
            "product",
            None,
        )?;
        let obs = read(&dir.join("product/obligations.json"))?;
        let [obligation] = obs["obligations"].as_array().unwrap().as_slice() else {
            panic!("expected one obligation")
        };
        assert_eq!(obligation["expected"]["copy"], copy);
        assert_eq!(obligation["expected"]["debug"], "derived-variant-name");
        assert_eq!(
            obligation["expected"].get("externalRepresentation"),
            Some(&Value::Null)
        );
        for path in ["choice.rs", "product/values.rs"] {
            library(&dir, "test_sample", path, "--edition=2024")?;
            caller(
                &dir,
                "test_sample",
                fixture!("bearing-caller.rs"),
                "--edition=2024",
                None,
            )?;
            assert_eq!(run(&dir, "./caller", &[], None)?, expected.as_bytes());
            trait_check(
                &dir,
                "test_sample",
                "Bearing",
                "Copy",
                copy,
                "--edition=2024",
            )?;
        }
        write(&dir.join("bad.json"), &graph)?;
        project(
            cli,
            &dir,
            "rust",
            "bad.json",
            "recovered/rust.json",
            "bad",
            Some("invalid plain choice native binding"),
        )?;
    }
    let base = fixture!("bearing.rs");
    for (source, version) in [
        (base.to_owned(), "v1"),
        (base.replace("Debug", "Debug, Serialize"), "v2"),
        (format!("use custom::Debug;\n{base}"), "v2"),
    ] {
        selection(
            cli,
            root,
            &source,
            "urn:fixture:route:choice",
            "Bearing",
            version,
            "bad",
            Some(""),
        )?;
    }
    println!("Both native variants match exact Debug, Eq, Clone, Copy and declaration ordinals.");
    Ok(())
}
pub fn equality(cli: &Path, root: &Path) -> Result {
    for (mode, derives, capability) in [
        ("none", "Clone, Copy", "none"),
        ("partial", "PartialEq, Copy, Clone", "partial-case-identity"),
        ("total", "Eq, Copy, PartialEq, Clone", "case-identity"),
    ] {
        let dir = root.join(mode);
        fs::create_dir(&dir)?;
        let source = format!("#[derive({derives})] pub enum Sampling {{ Nearest, Linear }}");
        selection(
            cli,
            &dir,
            &source,
            "urn:sampling",
            "Sampling",
            "v1",
            "bundle",
            None,
        )?;
        let graph = read(&dir.join("bundle/graph.json"))?;
        let types = graph["types"].as_object().unwrap();
        assert_eq!(types.len(), 1);
        let native = types.values().next().unwrap()["equality"].as_str();
        assert_eq!(native, (mode != "total").then_some(mode));
        project(
            cli,
            &dir,
            "rust",
            "bundle/graph.json",
            "bundle/rust.json",
            "product",
            None,
        )?;
        let obs = read(&dir.join("product/obligations.json"))?;
        let [obligation] = obs["obligations"].as_array().unwrap().as_slice() else {
            panic!("expected one obligation")
        };
        assert_eq!(obligation["expected"]["equality"], capability);
        for path in ["choice.rs", "product/values.rs"] {
            library(&dir, "sampling", path, EDITION)?;
            for (bound, expected) in [
                ("Copy", true),
                ("Clone", true),
                ("PartialEq", mode != "none"),
                ("Eq", mode == "total"),
            ] {
                trait_check(&dir, "sampling", "Sampling", bound, expected, EDITION)?;
            }
            if mode != "none" {
                caller(
                    &dir,
                    "sampling",
                    "use sampling::Sampling::*; fn main(){assert!(Nearest==Nearest && Nearest!=Linear);}",
                    EDITION,
                    None,
                )?;
                run(&dir, "./caller", &[], None)?;
            }
        }
    }
    println!("Three original and generated choice equality contracts passed.");
    Ok(())
}
pub fn legacy(old: &Path, new: &Path, root: &Path) -> Result {
    for (label, cli) in [("baseline", old), ("candidate", new)] {
        selection(
            cli,
            root,
            COMPASS,
            "urn:compass:v1",
            "Compass",
            "v1",
            label,
            None,
        )?;
        project(
            cli,
            root,
            "rust",
            &format!("{label}/graph.json"),
            &format!("{label}/rust.json"),
            &format!("{label}-rust"),
            None,
        )?;
    }
    for (suffix, file) in [
        ("", "graph.json"),
        ("", "rust.json"),
        ("-rust", "values.rs"),
        ("-rust", "obligations.json"),
    ] {
        let original = root.join(format!("baseline{suffix}/{file}"));
        assert_eq!(
            fs::read(&original)?,
            fs::read(root.join(format!("candidate{suffix}/{file}")))?
        );
        println!("{file} {}", sha(&original)?);
    }
    Ok(())
}
