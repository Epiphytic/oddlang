use super::*;

fn policy(s: &Suite, source: &Path) -> Result<Value> {
    let mut primary = s.recover_selected(
        "cli-surface",
        source,
        json!({"root":"Primary","package_name":"observatory","version":"2.1.0"}),
    )?;
    let aux = s.recover_selected(
        "cli-surface",
        source,
        json!({"root":"Survey","package_name":"observatory","version":""}),
    )?;
    let policy=s.recover_selected("cli-terminal-prefix",source,json!({"allow":"accept","route":"check","error":"invalid","supplied":"before","item":"field"}))?;
    primary["terminal_subparsers"] = json!({"survey":aux["root"]});
    primary["terminal_prefixes"] = json!({"survey":policy});
    Ok(primary)
}
pub(super) fn verify(s: &Suite) -> Result {
    let text = fs::read_to_string(s.fixture.join("prefix-reference.rs"))?;
    let source = s.auxiliary(&text)?;
    let binding = s.root.join("binding.json");
    write(
        &binding,
        &json!({"executable":"meter","invalid_exit":18,"invalid_diagnostic":"meter: invalid arguments","uncovered_exit":125,"operations":{}}),
    )?;
    let graph = s.root.join("graph.json");
    let emitted = s.root.join("src/cli.rs");
    let reference = s.root.join("target/debug/reference");
    let candidate = s.root.join("target/debug/candidate");
    let cases = s.cases("prefix")?;
    let mut primary = Value::Null;
    let revised = text.replace("\"place\" | \"brief\"", "\"place\" | \"brief\" | \"agent\"");
    for (revision, agent_status) in [(&text, 18), (&revised, 0)] {
        fs::write(&source, revision)?;
        primary = policy(s, &source)?;
        write(&graph, &primary)?;
        s.project(&graph, &binding, &emitted, None)?;
        s.cargo(&s.root, "build")?;
        for case in &cases {
            let args: Vec<_> = case.iter().map(String::as_str).collect();
            s.compare(&reference, &candidate, &args)?;
        }
        assert_eq!(
            s.observe(&candidate, &["--agent=east", "survey", "--help"])?
                .status,
            agent_status
        );
        let accepted = s.observe(&candidate, &["--brief", "survey", "label"])?;
        assert!(accepted.status == 18 && accepted.stderr_has("unexpected argument 'label'"));
        let flags = ["-p", "--brief", "survey", "--help"];
        let raw = s.observe(&reference, &flags)?;
        assert!(raw.status == 0 && raw.stdout_has("Survey readings"));
        let fallback = s.observe(&candidate, &flags)?;
        assert!(
            fallback.status == 18
                && fallback.stdout.is_empty()
                && fallback.stderr_has("a value is required for '--place")
        );
        if agent_status == 18 {
            let denied = s.compare(&reference, &candidate, &["--agent=east", "survey", "label"])?;
            assert!(denied.stderr_has("meter: option rejected"));
        }
    }
    let original_policy = primary["terminal_prefixes"]["survey"].clone();
    let denied = |s: &Suite, doc: &Value| -> Result {
        write(&graph, doc)?;
        s.project(
            &graph,
            &binding,
            &s.root.join("invalid.rs"),
            Some("invalid terminal prefix policy"),
        )
    };
    primary["terminal_prefixes"]["survey"]["allowed"]
        .as_array_mut()
        .unwrap()
        .push(json!("missing-option"));
    denied(s, &primary)?;
    for (from, to, message) in [
        (
            "matches!(field, \"place\" | \"brief\")",
            "field.starts_with(\"p\")",
            "expected a literal allowlist predicate",
        ),
        (
            "\"place\" | \"brief\"",
            "\"place\" | \"place\"",
            "duplicate allowed option identifier",
        ),
        (
            "!accept(field)",
            "accept(field)",
            "expected one direct rejected-prefix branch",
        ),
        (
            "ErrorKind::InvalidValue",
            "ErrorKind::UnknownArgument",
            "unsupported raw diagnostic format",
        ),
    ] {
        assert!(text.contains(from));
        fs::write(&source, text.replace(from, to))?;
        s.recover(
            "cli-terminal-prefix",
            &source,
            &s.root.join("selection.json"),
            &graph,
            Some(message),
        )?;
    }
    primary["terminal_prefixes"] = json!({"absent":original_policy});
    denied(s, &primary)?;
    primary["terminal_prefixes"] = json!({"survey":original_policy});
    primary["terminal_prefixes"]["survey"]["denied"] = json!("invalid\ntext");
    denied(s, &primary)
}
