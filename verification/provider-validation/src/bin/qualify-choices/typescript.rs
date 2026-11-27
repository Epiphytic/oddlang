use super::*;
const SKY: &str = "urn:fixture:survey:choices:Sky";
const SLOPE: &str = "urn:choice-fixture:slope:type";
fn tsc(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut flags = vec!["--offline", "--package=typescript@5.9.3", "tsc"];
    flags.extend_from_slice(args);
    run(root, "npx", &flags, None)
}
fn compare(
    cli: &Path,
    root: &Path,
    sky: &[&str],
    graph: &str,
    binding: &str,
    reversed: &str,
    native: &str,
) -> Result<Value> {
    fs::create_dir(root)?;
    fs::write(root.join("graph.json"), graph)?;
    fs::write(root.join("typescript.json"), binding)?;
    project(
        cli,
        root,
        "typescript",
        "graph.json",
        "typescript.json",
        "ts-first",
        None,
    )?;
    fs::write(root.join("typescript.json"), reversed)?;
    project(
        cli,
        root,
        "typescript",
        "graph.json",
        "typescript.json",
        "ts-second",
        None,
    )?;
    let out = root.join("ts-first");
    for file in ["values.ts", "obligations.json", "projection.lock"] {
        assert_eq!(
            fs::read(out.join(file))?,
            fs::read(root.join("ts-second").join(file))?
        );
    }
    let lock = read(&out.join("projection.lock"))?;
    for (key, file) in [
        ("sourceSha256", "values.ts"),
        ("obligationsSha256", "obligations.json"),
    ] {
        assert_eq!(lock[key], sha(&out.join(file))?);
    }
    fs::write(root.join("rust.json"), native)?;
    project(cli, root, "rust", "graph.json", "rust.json", "native", None)?;
    assert_eq!(
        fs::read(root.join("native/obligations.json"))?,
        fs::read(out.join("obligations.json"))?
    );
    fs::write(
        root.join("reference.rs"),
        fixture!("choice-observer.rs.in").replace("INPUT", &sky.join(", ")),
    )?;
    run(
        root,
        "rustc",
        &["--edition=2024", "reference.rs", "-o", "reference"],
        None,
    )?;
    let reference = run(root, "./reference", &[], None)?;
    fs::write(out.join("observer.ts"), fixture!("observer.ts"))?;
    fs::write(out.join("negative.ts"), fixture!("negative.ts"))?;
    tsc(
        &out,
        &[
            "--strict",
            "--target",
            "ES2022",
            "--module",
            "commonjs",
            "--noEmit",
            "negative.ts",
        ],
    )?;
    tsc(
        &out,
        &[
            "--strict",
            "--target",
            "ES2022",
            "--module",
            "commonjs",
            "--declaration",
            "--outDir",
            "compiled",
            "observer.ts",
        ],
    )?;
    let observed = run(&out, "node", &["compiled/observer.js"], None)?;
    assert_eq!(reference, observed);
    Ok(serde_json::from_slice(&observed)?)
}
pub fn check(cli: &Path, root: &Path) -> Result {
    assert_eq!(
        String::from_utf8(tsc(root, &["--version"])?)?.trim(),
        "Version 5.9.3"
    );
    let baseline = compare(
        cli,
        &root.join("baseline"),
        &["Clear", "Mist", "Cloud"],
        fixture!("baseline-graph.json"),
        fixture!("baseline-typescript.json"),
        fixture!("baseline-typescript-reversed.json"),
        fixture!("baseline-rust.json"),
    )?;
    let changed = compare(
        cli,
        &root.join("changed"),
        &["Cloud", "Clear", "Mist"],
        fixture!("changed-graph.json"),
        fixture!("changed-typescript.json"),
        fixture!("changed-typescript-reversed.json"),
        fixture!("changed-rust.json"),
    )?;
    assert_eq!(baseline["Sky"], json!([0, 1, 2]));
    assert_eq!(changed["Sky"], json!([1, 2, 0]));
    for id in ["Side", "Tier"] {
        assert_eq!(baseline[id], changed[id]);
    }
    let graph: Value = serde_json::from_str(fixture!("baseline-graph.json"))?;
    let binding: Value = serde_json::from_str(fixture!("baseline-typescript.json"))?;
    let mut duplicate = graph.clone();
    duplicate["types"][SKY]["cases"][2] = graph["types"][SKY]["cases"][0].clone();
    let mut invalid = binding.clone();
    invalid["types"][SKY] = json!("arbitrary text");
    let mut unused = binding.clone();
    unused["variants"]["urn:fixture:survey:choices:Unknown"] = json!("Unknown");
    for (g, b, error) in [
        (&duplicate, &binding, "repeated checked value identity"),
        (&graph, &invalid, "invalid TypeScript choice name"),
        (&graph, &unused, "unused TypeScript choice case"),
    ] {
        write(&root.join("graph.json"), g)?;
        write(&root.join("typescript.json"), b)?;
        project(
            cli,
            root,
            "typescript",
            "graph.json",
            "typescript.json",
            "invalid-output",
            Some(error),
        )?;
    }
    println!(
        "Rust and TypeScript agree on both choice revisions, capability checks and exact ordinals."
    );
    Ok(())
}
pub fn subset(cli: &Path, older: Option<&Path>, root: &Path) -> Result {
    let case = format!("{SLOPE}:Below");
    let graph = json!({"format":"oddlang-checked-values-v1","id":"urn:choice-fixture:slope","types":{SLOPE:{"kind":"choice","cases":[case]}}});
    write(&root.join("graph.json"), &graph)?;
    write(
        &root.join("typescript.json"),
        &json!({"format":"oddlang-checked-values-typescript-v1","types":{SLOPE:"Slope"},"variants":{case:"Below"}}),
    )?;
    project(
        cli,
        root,
        "typescript",
        "graph.json",
        "typescript.json",
        "plain",
        None,
    )?;
    assert!(root.join("plain/values.ts").is_file());
    if let Some(old) = older {
        project(
            old,
            root,
            "typescript",
            "graph.json",
            "typescript.json",
            "previous",
            None,
        )?;
        for file in ["values.ts", "obligations.json", "projection.lock"] {
            let path = root.join("plain").join(file);
            assert_eq!(
                fs::read(&path)?,
                fs::read(root.join("previous").join(file))?
            );
            println!("{file} {}", sha(&path)?);
        }
    }
    for (label, field, value) in [
        ("clone-only", "copy", false),
        ("derived-debug", "debug", true),
    ] {
        let mut g = graph.clone();
        g["types"][SLOPE][field] = json!(value);
        write(&root.join("graph.json"), &g)?;
        project(
            cli,
            root,
            "typescript",
            "graph.json",
            "typescript.json",
            label,
            Some(&format!(
                "TypeScript checked choices do not support {SLOPE}"
            )),
        )?;
    }
    println!(
        "TypeScript projects the plain choice and rejects clone-only and derived Debug choices."
    );
    Ok(())
}
