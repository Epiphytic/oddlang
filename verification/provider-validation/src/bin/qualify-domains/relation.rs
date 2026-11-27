use super::*;

fn modify(value: &mut Value, edits: &Value) {
    for edit in edits.as_array().unwrap() {
        let ptr = edit[0].as_str().unwrap();
        let target = value.pointer_mut(ptr).expect("fixture path");
        if let Some(key) = edit[1].as_str() {
            target[key] = edit[2].clone();
        } else {
            *target = edit[2].clone();
        }
    }
}
pub fn verify(cli: &Path, root: &Path) -> Result {
    let graph = fixture("relation.json")?;
    let overlay = fixture("relation-rust.json")?;
    let project = |g: &Value, r: &Value, out: &str, error: Option<&str>| -> Result {
        write(&root.join("graph.json"), g)?;
        write(&root.join("rust.json"), r)?;
        build(
            cli,
            root,
            "field-relation",
            "graph.json",
            "rust.json",
            out,
            error,
        )
    };
    project(&graph, &overlay, "out", None)?;
    fs::copy(
        fixtures().join("relation-caller.rs"),
        root.join("out/main.rs"),
    )?;
    command(
        &mut Command::new("rustc"),
        root,
        &["--edition=2024", "out/main.rs", "-o", "witness"],
        None,
    )?;
    command(&mut Command::new("./witness"), root, &[], None)?;
    for control in fixture("relation-rejections.json")?.as_array().unwrap() {
        let mut g = graph.clone();
        let mut b = overlay.clone();
        modify(&mut g, &control["graph"]);
        modify(&mut b, &control["binding"]);
        project(&g, &b, "rejected", Some(control["error"].as_str().unwrap()))?;
    }
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='payment-fixture'\nversion='0.0.0'\nedition='2024'\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    let mut parts = Vec::new();
    for (kind, module, exports) in [
        ("shared-declarations", "fault", &["WireFault"][..]),
        ("binary-record", "invoice", &["Invoice"]),
        ("binary-record", "remittance", &["Remittance"]),
        ("field-relation", "relation", &[]),
    ] {
        parts.push(component(
            root,
            kind,
            module,
            exports,
            fixture(&format!("package-{module}.json"))?,
            fixture(&format!("package-{module}-rust.json"))?,
        )?);
    }
    let mut spec = package("payment-fixture", "0.0.0", "2024", root, json!(parts))?;
    assemble(cli, root, &spec, "package", None)?;
    let product = root.join("package");
    fs::create_dir(product.join("examples"))?;
    fs::copy(
        fixtures().join("payment-caller.rs"),
        product.join("examples/witness.rs"),
    )?;
    cargo(
        &product,
        &["run", "--locked", "--offline", "--example", "witness"],
    )?;
    spec["components"][3]["reexports"] = json!(["Invoice"]);
    assemble(cli, root, &spec, "bad-reexports", Some("cannot reexport"))?;
    println!("Typed field equality and ordered checked calls pass independent runtime controls.");
    Ok(())
}
