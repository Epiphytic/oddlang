use oddlang::tagged_wire::load;
use std::{fs, path::Path, process::Command};

const GRAPH: &str = "tests/fixtures/tagged-wire/wire.ttl";

fn run_node(graph: &Path, script: &str) {
    let contract = load(graph).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let generated = temp.path().join("generated");
    let output = Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["build", "tagged-wire", "typescript"])
        .arg(graph)
        .arg(&generated)
        .args(["--graph-sha256", contract.sha256()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new("node")
        .args(["--experimental-strip-types", script])
        .arg(generated.join("wire.mts"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn emitted_typescript_preserves_u64_and_ordered_wire_errors() {
    run_node(Path::new(GRAPH), "tests/support/tagged-wire.mjs");
}

#[test]
fn emitted_typescript_stream_stops_at_each_record_and_poisoned_streams_stay_failed() {
    let temp = tempfile::tempdir().unwrap();
    let graph = temp.path().join("stream.ttl");
    let source = fs::read_to_string(GRAPH).unwrap();
    let stream = r#"
e:Packet t:feedFailed e:failed ; t:incomplete e:incomplete .
e:failed t:code "Failed" .
e:incomplete t:code "Incomplete" .
"#;
    fs::write(&graph, format!("{source}{stream}")).unwrap();
    run_node(&graph, "tests/support/tagged-wire-stream.mjs");
}
