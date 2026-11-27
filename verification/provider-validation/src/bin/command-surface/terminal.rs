use super::*;
pub(super) fn verify(s: &Suite) -> Result {
    let text = fs::read_to_string(s.fixture.join("terminal-reference.rs"))?;
    let source = s.auxiliary(&text)?;
    let graph = s.root.join("graph.json");
    s.base(&graph)?;
    let binding = s.fixture.join("binding.json");
    let output = s.root.join("src/cli.rs");
    let mut primary = read(&graph)?;
    primary["root"]["subcommands"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"survey"}));
    let cases = s.cases("terminal")?;
    let reference = s.root.join("target/debug/reference");
    let candidate = s.root.join("target/debug/candidate");
    let revised = text
        .replace("1..=9", "1..=8")
        .replace("Inspect sample labels", "Review survey labels");
    let mut observations = Vec::new();
    for revision in [&text, &revised] {
        fs::write(&source, revision)?;
        let aux = s.recover_selected(
            "cli-surface",
            &source,
            json!({"root":"Survey","package_name":"observatory","version":""}),
        )?;
        primary["terminal_subparsers"] = json!({"survey":aux["root"]});
        write(&graph, &primary)?;
        s.project(&graph, &binding, &output, None)?;
        s.cargo(&s.root, "build")?;
        let mut rows = Vec::new();
        for case in &cases {
            let mut args = vec!["survey"];
            args.extend(case.iter().map(String::as_str));
            rows.push(s.compare(&reference, &candidate, &args)?);
        }
        observations.push(rows);
    }
    let [baseline, mutant] = observations.as_slice() else {
        unreachable!()
    };
    assert_eq!(baseline[0].status, 0);
    assert_eq!(mutant[0].status, 0);
    assert_ne!(baseline[0].stdout, mutant[0].stdout);
    assert_eq!(baseline[4].status, 125);
    assert_eq!(mutant[4].status, 12);
    for row in &baseline[2..] {
        assert!(row.status == 0 || row.stdout.is_empty());
    }
    assert!(baseline[4].stdout.is_empty() && !baseline[4].stderr_has("invalid arguments"));
    let reject = |doc: &Value, overlay: &Path, message: &str| -> Result {
        write(&graph, doc)?;
        s.project(&graph, overlay, &output, Some(message))
    };
    primary["terminal_subparsers"]["unregistered"] =
        primary["terminal_subparsers"]["survey"].clone();
    reject(&primary, &binding, "unknown terminal parser trigger")?;
    primary["terminal_subparsers"]
        .as_object_mut()
        .unwrap()
        .remove("unregistered");
    let mut dispatch = read(&binding)?;
    dispatch["operations"]["survey"] = json!("urn:observatory:survey");
    let bound = s.root.join("binding.json");
    write(&bound, &dispatch)?;
    reject(&primary, &bound, "invalid terminal parser")?;
    primary["root"]["subcommands"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["aliases"] = json!(["inspect"]);
    reject(&primary, &binding, "invalid terminal parser")?;
    fs::write(&source, text.replace("num_args = 1..", "num_args = 2.."))?;
    s.recover(
        "cli-surface",
        &source,
        &s.root.join("selection.json"),
        &s.root.join("auxiliary.json"),
        Some("one-or-more variadic argument counts"),
    )
}
