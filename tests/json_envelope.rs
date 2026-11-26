use std::{fs, path::Path, process::Command};

const GRAPH: &str = include_str!("fixtures/json-envelope/graph/domain.ttl");
const BINDING: &str = include_str!("fixtures/json-envelope/binding.json");
const CONSUMER: &str = include_str!("fixtures/json-envelope/consumer.rs");
const MANIFEST: &str = include_str!("fixtures/json-envelope/Cargo.toml");
const LOCK: &str = include_str!("fixtures/json-envelope/Cargo.lock");

fn project(root: &Path, name: &str, graph: &str) -> std::process::Output {
    let graph_dir = root.join(format!("{name}-graph"));
    fs::create_dir(&graph_dir).unwrap();
    fs::write(graph_dir.join("domain.ttl"), graph).unwrap();
    let binding = graph_dir.join("binding.json");
    fs::write(&binding, BINDING).unwrap();
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["build", "json-envelope", "rust"])
        .arg(graph_dir)
        .arg(binding)
        .arg(root.join(name))
        .output()
        .unwrap()
}

#[test]
fn generic_envelope_produces_a_native_codec_for_an_unchanged_consumer() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let renamed = GRAPH
        .replace("position 0", "position 2")
        .replace("position 1", "position 0")
        .replace("position 2", "position 1")
        .replace("wireName \"rev\"", "wireName \"revision\"")
        .replace("wireName \"pixels\"", "wireName \"octets\"");
    let tightened = GRAPH
        .replace("rawBytes 4", "rawBytes 1")
        .replace("wireBytes 52", "wireBytes 22");
    let reordered = GRAPH.replace("json literal encoded-size", "json encoded-size literal");
    let tuple = GRAPH.replace("ClosedRequiredObject", "ClosedRequiredObjectOrTuple");
    let equivalent = GRAPH
        .replace(
            "image:revision, image:pixels",
            "image:pixels, image:revision",
        )
        .replace(
            "c:rawBytes 4 ; c:wireBytes 52",
            "c:wireBytes 52 ; c:rawBytes 4",
        );
    for (name, graph) in [
        ("baseline", GRAPH),
        ("replica", &equivalent),
        ("renamed", &renamed),
        ("tightened", &tightened),
        ("reordered", &reordered),
        ("tuple", &tuple),
    ] {
        let output = project(root, name, graph);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if name != "replica" {
            fs::copy(
                root.join(name).join("codec.rs"),
                root.join(format!("{name}.rs")),
            )
            .unwrap();
        }
    }
    for name in ["codec.rs", "projection.lock"] {
        assert_eq!(
            fs::read(root.join("baseline").join(name)).unwrap(),
            fs::read(root.join("replica").join(name)).unwrap()
        );
    }
    fs::write(root.join("consumer.rs"), CONSUMER).unwrap();
    fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
    fs::write(root.join("Cargo.lock"), LOCK).unwrap();
    let output = Command::new(env!("CARGO"))
        .args(["test", "--locked", "--offline", "--quiet"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn malformed_relationships_and_unsafe_guard_order_are_rejected() {
    let scratch = tempfile::tempdir().unwrap();
    for (name, graph) in [
        (
            "duplicate-position",
            GRAPH.replace("position 1", "position 0"),
        ),
        (
            "invalid-order",
            GRAPH.replace("wire-size json literal", "json wire-size literal"),
        ),
        (
            "duplicate-field",
            GRAPH.replace("wireName \"pixels\"", "wireName \"rev\""),
        ),
        (
            "out-of-width",
            GRAPH.replace("c:value 13", "c:value 4294967296"),
        ),
    ] {
        let output = project(scratch.path(), name, &graph);
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            !scratch.path().join(name).exists(),
            "published incomplete output"
        );
    }
}
