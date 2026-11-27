use super::*;
use std::collections::BTreeMap;
const FAULT: &str = "urn:example:detector:invalid";
fn id(name: &str) -> String {
    format!("urn:example:detector:{name}")
}
fn values(root: &Path, g: Value, r: Value) -> Result<Value> {
    component(
        root,
        "checked-values",
        "values",
        &["Caption", "Metadata", "Octave", "Station", "Tone"],
        g,
        r,
    )
}
fn witness(cli: &Path, root: &Path, spec: &Value, label: &str, first: bool) -> Result {
    assemble(cli, root, spec, label, None)?;
    let dir = root.join(label);
    command(
        Command::new("cargo").env("CARGO_TARGET_DIR", dir.join("target")),
        &dir,
        &["build", "--locked", "--offline"],
        None,
    )?;
    fs::copy(
        fixtures().join("detector-caller.rs"),
        dir.join("witness.rs"),
    )?;
    command(
        &mut Command::new("rustc"),
        &dir,
        &[
            "--edition=2024",
            "witness.rs",
            "--extern",
            "detector=target/debug/libdetector.rlib",
            "-o",
            "witness",
        ],
        None,
    )?;
    let output = String::from_utf8(command(&mut Command::new("./witness"), &dir, &[], None)?)?;
    let expected = [
        first, true, false, false, false, false, false, false, false, false, true, true,
    ];
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        expected.map(|b| b.to_string())
    );
    Ok(())
}
fn finite(root: &Path, payload: &str) -> Result<Value> {
    let message = if payload == "unit" {
        json!({"kind":"literal","text":"bad detector input"})
    } else {
        json!({"kind":"insertion","prefix":"bad input: ","suffix":""})
    };
    let graph = json!({"format":"oddlang-finite-error-domain-v1","id":id("finite"),"error":id("fault"),"result":id("result"),"categories":{},
        "variants":{FAULT:{"payload":{"kind":payload},"message":message,"source":false,"from":false}}});
    let rust = json!({"format":"oddlang-finite-error-rust-v1","variantOrder":[FAULT],"categoryTypes":{},"names":{(id("fault")):"Fault",(id("result")):"Result",FAULT:"Invalid"}});
    let mut c = component(
        root,
        "finite-error-domain",
        "finite",
        &["Fault"],
        graph,
        rust,
    )?;
    c["rootReexports"] = json!([]);
    Ok(c)
}
pub fn verify(cli: &Path, root: &Path) -> Result {
    let graph = fixture("detector.json")?;
    let binding = fixture("detector-rust.json")?;
    let shared = component(
        root,
        "shared-declarations",
        "faults",
        &["Fault"],
        fixture("errors.json")?,
        fixture("error-rust.json")?,
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"detector\"\nversion = \"0.0.0\"\n",
    )?;
    let spec = |parts: Value| package("detector", "0.0.0", "2024", root, parts);
    let val = values(root, graph.clone(), binding.clone())?;
    witness(cli, root, &spec(json!([shared, val]))?, "baseline", true)?;
    let linked = |key: &str, value: &str| -> Result<Value> {
        let mut r = binding.clone();
        r["errors"][FAULT][key] = json!(value);
        write(&root.join("linked-rust.json"), &r)?;
        let mut linked = val.clone();
        linked["overlay"] = pin(root, "linked-rust.json")?;
        Ok(linked)
    };
    let attempt = |name: &str, parts: Value, error: Option<&str>| -> Result {
        assemble(cli, root, &spec(parts)?, name, error)?;
        if error.is_none() {
            cargo(&root.join(name), &["check", "--locked", "--offline"])?;
        }
        Ok(())
    };
    let mismatch = format!("checked value error {FAULT} has no matching unit export");
    attempt(
        "misbound",
        json!([shared, linked("variant", "Other")?]),
        Some(&mismatch),
    )?;
    let mut module = shared.clone();
    module["rootReexports"] = json!([]);
    attempt(
        "module",
        json!([module, linked("errorType", "crate::faults::Fault")?]),
        None,
    )?;
    attempt("unexported-root", json!([module, val]), Some(&mismatch))?;
    attempt(
        "finite",
        json!([
            finite(root, "unit")?,
            linked("errorType", "crate::finite::Fault")?
        ]),
        None,
    )?;
    let owned = finite(root, "owned-text")?;
    attempt(
        "payload",
        json!([owned, linked("errorType", "crate::finite::Fault")?]),
        Some(&mismatch),
    )?;
    attempt(
        "ambiguous",
        json!([shared, owned, val]),
        Some(&format!("ambiguous native error declaration: {FAULT}")),
    )?;

    let project = |g: &Value, r: &Value, out: &str, error: Option<&str>| -> Result {
        write(&root.join("graph.json"), g)?;
        write(&root.join("rust.json"), r)?;
        build(
            cli,
            root,
            "checked-values",
            "graph.json",
            "rust.json",
            out,
            error,
        )
    };
    project(&graph, &binding, "first", None)?;
    let mut reversed = binding.clone();
    reversed["types"][id("tone")]["traits"]
        .as_array_mut()
        .unwrap()
        .reverse();
    project(&graph, &reversed, "second", None)?;
    for name in ["values.rs", "obligations.json", "projection.lock"] {
        assert_eq!(
            fs::read(root.join("first").join(name))?,
            fs::read(root.join("second").join(name))?
        );
    }
    let lock = read(&root.join("first/projection.lock"))?;
    assert_eq!(lock["format"], "oddlang-checked-values-lock-v1");
    assert_eq!(
        lock["obligationsSha256"],
        sha(&root.join("first/obligations.json"))?
    );
    assert_eq!(lock["sourceSha256"], sha(&root.join("first/values.rs"))?);
    let obligations = read(&root.join("first/obligations.json"))?;
    let mut cases = BTreeMap::new();
    for row in obligations["obligations"].as_array().unwrap() {
        assert_eq!(row["role"], "checked-value-domain");
        assert_eq!(row["qualification"], "independent-witness-required");
        assert!(
            cases
                .insert(row["declaration"].as_str().unwrap(), &row["expected"])
                .is_none()
        );
    }
    assert_eq!(
        cases.keys().copied().collect::<Vec<_>>(),
        graph["types"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        cases[id("caption").as_str()]["orderedGuards"],
        graph["types"][id("caption")]["guards"]
    );
    assert_eq!(
        cases[id("tone").as_str()]["values"],
        graph["types"][id("tone")]["values"]
    );
    let mut mutated = graph.clone();
    mutated["types"][id("caption")]["guards"][0]["maximum"] = json!(5);
    witness(
        cli,
        root,
        &spec(json!([shared, values(root, mutated, binding.clone())?]))?,
        "mutated",
        false,
    )?;
    for (label, error) in [
        ("cycle", "recursive"),
        ("public", "invalid checked field"),
        ("inaccessible", "invalid checked record"),
        ("trait-shadow", "invalid finite value table bindings"),
        ("borrow-shadow", "invalid checked field"),
        ("unreadable-checked", "invalid checked field"),
        ("mixed", "invalid or repeated representation"),
        ("overlap", "ill-typed constructor guard"),
    ] {
        let mut g = graph.clone();
        let mut r = binding.clone();
        match label {
            "cycle" => {
                g["types"][id("metadata")]["fields"][id("metadata:station")]["id"] =
                    json!(id("metadata"))
            }
            "public" => {
                r["types"][id("station")]["members"][id("station:code")]["public"] = json!(true)
            }
            "inaccessible" => {
                r["types"][id("metadata")]["members"][id("metadata:caption")]["public"] =
                    json!(false)
            }
            "trait-shadow" => r["types"][id("tone")]["view"] = json!("clone"),
            "borrow-shadow" => {
                r["types"][id("station")]["members"][id("station:code")]["getter"] = json!("borrow")
            }
            "unreadable-checked" => {
                r["types"][id("station")]["members"][id("station:code")]
                    .as_object_mut()
                    .unwrap()
                    .remove("getter");
            }
            "mixed" => g["types"][id("octave")]["values"][id("low")] = json!("one"),
            "overlap" => g["types"][id("station")]["guards"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["ranges"]
                .as_array_mut()
                .unwrap()
                .push(json!({"minimum":90,"maximum":95})),
            _ => unreachable!(),
        }
        project(&g, &r, "rejected", Some(error))?;
    }
    println!(
        "Qualified unrelated checked records, enum tables, error categories and a native bound mutation."
    );
    Ok(())
}
