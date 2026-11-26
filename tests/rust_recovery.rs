use std::{fs, process::Command};

#[test]
fn recovery_rejects_undeclared_relationships_without_publishing() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.json");
    fs::write(
        &input,
        r#"{"crateName":"sample","declarations":[{"id":"local::sample","name":"sample","kind":"crate","owner":null,"visibility":"public","macroGenerated":false,"line":null,"column":null,"file":null}],"edges":[{"from":"local::sample","relation":"inject-code","to":"local::sample"}],"gaps":[]}"#,
    )
    .unwrap();
    let output = temp.path().join("release");
    let feedback = Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["recover", "validate-inventory"])
        .arg(input)
        .arg(output.as_path())
        .output()
        .unwrap();
    assert!(!feedback.status.success());
    let message = String::from_utf8_lossy(&feedback.stderr);
    assert!(
        message.contains("unknown variant `inject-code`"),
        "{message}"
    );
    assert!(!output.exists());
}

#[test]
fn candidate_inventory_serializes_named_facts_in_canonical_order() {
    let temp = tempfile::tempdir().unwrap();
    let mut inventory = serde_json::json!({
        "crateName": "sample",
        "declarations": [
            {"id":"local::sample","name":"sample","kind":"crate","owner":null,"visibility":"public","macroGenerated":false},
            {"id":"local::sample::first","name":"first","kind":"function","owner":"local::sample","visibility":"private","macroGenerated":false},
            {"id":"local::sample::second","name":"second","kind":"function","owner":"local::sample","visibility":"private","macroGenerated":false},
        ],
        "edges": [
            {"from":"local::sample::first","relation":"call","to":"local::sample::second"},
            {"from":"local::sample::second","relation":"call","to":"external::std::mem::drop"},
        ],
        "gaps":[{"owner":"local::sample::second","kind":"indirect-call"}],
    });
    let input = temp.path().join("inventory.json");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let build = |inventory: &serde_json::Value, out: &std::path::Path| {
        fs::write(&input, serde_json::to_vec(inventory).unwrap()).unwrap();
        oddlang::rust_recovery::validate_inventory(&input, out)
    };
    build(&inventory, &first).unwrap();
    inventory["declarations"].as_array_mut().unwrap().reverse();
    inventory["edges"].as_array_mut().unwrap().reverse();
    build(&inventory, &second).unwrap();
    for name in ["structure.ttl", "structure.lock"] {
        assert_eq!(
            fs::read(first.join(name)).unwrap(),
            fs::read(second.join(name)).unwrap()
        );
    }
    let graph = fs::read_to_string(first.join("structure.ttl")).unwrap();
    assert!(graph.contains(":call> "));
    assert!(graph.contains("indirect-call"));
    assert!(graph.contains("external::std::mem::drop"));
    let rejected = temp.path().join("rejected");
    inventory["declarations"][0]["name"] = serde_json::json!("fn injected() {}");
    assert!(
        build(&inventory, &rejected)
            .unwrap_err()
            .contains("invalid descriptor")
    );
    inventory["declarations"][0]["name"] = serde_json::json!("second");
    inventory["declarations"][0]["tokens"] = serde_json::json!(["fn", "injected"]);
    assert!(
        build(&inventory, &rejected)
            .unwrap_err()
            .contains("unknown field `tokens`")
    );
    assert!(!rejected.exists());
}
