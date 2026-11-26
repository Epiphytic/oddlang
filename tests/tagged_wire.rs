use oddlang::tagged_wire::{Value, load};
use std::{fs, path::Path};

const GRAPH: &str = "tests/fixtures/tagged-wire/wire.ttl";

#[test]
fn bounded_tagged_wire_uses_lossless_u64_and_preserves_guard_order() {
    let wire = load(Path::new(GRAPH)).unwrap();
    let high = u64::MAX - 2;
    let sample = [Value::UInt(high), Value::Bytes(vec![255])];
    let expected = [42, 0, 0, 0, 9, 255, 255, 255, 255, 255, 255, 255, 253, 255];
    assert_eq!(wire.encode(42, &sample).unwrap(), expected);
    assert_eq!(wire.decode(&expected).unwrap(), (42, sample.into()));
    assert_eq!(wire.variant_name(42), Some("Chunk"));
    assert_eq!(
        wire.field_names(42).unwrap().collect::<Vec<_>>(),
        ["index", "data"]
    );

    let overlap = [Value::UInt(u64::MAX), Value::Bytes(vec![])];
    assert_eq!(wire.encode(42, &overlap), Err("Limit"));
    assert_eq!(
        wire.encode(42, &[Value::UInt(u64::MAX), Value::Bytes(vec![0])]),
        Err("Range")
    );
    assert_eq!(
        wire.decode(&[42, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255]),
        Err("Range")
    );
    assert_eq!(wire.decode(&[255, 0, 0, 0, 26]), Err("Limit"));
    assert_eq!(wire.decode(&[62, 0, 0, 0, 1, 1]), Err("Truncated"));
    assert_eq!(wire.decode(&[62, 0, 0, 0, 4, 1, 2, 3, 4]), Err("Trailing"));
}

#[test]
fn pins_ignore_duplicate_triples_and_reject_unknown_properties() {
    let original = load(Path::new(GRAPH)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("wire.ttl");
    let source = fs::read_to_string(GRAPH).unwrap();
    fs::write(&file, format!("{source}\ne:Ping t:tag 62 .\n")).unwrap();
    assert_eq!(load(&file).unwrap().sha256(), original.sha256());
    let lexical = source.replace("t:tag 62", "t:tag +062");
    fs::write(&file, lexical).unwrap();
    assert!(load(&file).err().unwrap().contains("noncanonical integer"));

    fs::write(
        &file,
        format!("{source}\ne:Chunk t:body \"fn secret() {{}}\" .\n"),
    )
    .unwrap();
    assert!(
        load(&file)
            .err()
            .unwrap()
            .contains("unexpected tagged wire field body")
    );
}

#[test]
fn incremental_decoder_consumes_one_record_and_retains_failure() {
    let temp = tempfile::tempdir().unwrap();
    let graph = temp.path().join("stream.ttl");
    let source = fs::read_to_string(GRAPH).unwrap();
    fs::write(
        &graph,
        format!("{source}\ne:Packet t:feedFailed e:invalid ; t:incomplete e:truncated .\n"),
    )
    .unwrap();
    let wire = load(&graph).unwrap();
    let mut decoder = wire.decoder().unwrap();
    let first = [62, 0, 0, 0, 3, 1, 2, 3];
    let second = [62, 0, 0, 0, 3, 4, 5, 6];
    let part = decoder.feed(&first[..3]).unwrap();
    assert_eq!(part.consumed, 3);
    assert_eq!(part.record, None);
    assert_eq!(decoder.finish(), Err("Truncated"));
    let rest = decoder.feed(&[&first[3..], &second[..]].concat()).unwrap();
    assert_eq!(rest.consumed, first.len() - 3);
    assert_eq!(rest.record, Some((62, vec![Value::Bytes(vec![1, 2, 3])])));
    assert_eq!(
        decoder.feed(&second).unwrap().record,
        Some((62, vec![Value::Bytes(vec![4, 5, 6])]))
    );
    assert_eq!(decoder.finish(), Ok(()));
    assert_eq!(decoder.feed(&[62, 0, 0, 0, 26]), Err("Limit"));
    assert_eq!(decoder.feed(&second), Err("Invalid"));
    assert_eq!(decoder.finish(), Err("Truncated"));
}

#[test]
fn rejects_unreachable_and_inconsistent_contract_facts() {
    let source = fs::read_to_string(GRAPH).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let graph = temp.path().join("wire.ttl");
    let cases = [
        (
            format!("{source}\ne:orphan t:code \"Orphan\" .\n"),
            "unexpected tagged wire field",
        ),
        (
            source.replace("t:tag 62", "t:tag 42"),
            "duplicate variant name or tag",
        ),
        (
            source
                .replace("e:chunkData t:position 1", "e:chunkData t:position 0")
                .replace("e:chunkIndex t:position 0", "e:chunkIndex t:position 1"),
            "rest-bytes field must be last",
        ),
        (
            source.replace("t:sumWith e:chunkIndex", "t:sumWith e:nonce"),
            "sumWith must refer to an earlier field",
        ),
        (
            source.replace(
                "t:max 18446744073709551614 ; t:rangeError e:range",
                "t:rangeError e:range",
            ),
            "sumWith requires a ranged integer",
        ),
    ];
    for (source, message) in cases {
        fs::write(&graph, source).unwrap();
        let error = load(&graph).err().unwrap();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn source_to_graph_materializes_tag_and_checks_reviewed_fields() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("selected.rs");
    let selection = temp.path().join("selection.json");
    let overlay = temp.path().join("overlay.ttl");
    let out = temp.path().join("complete.ttl");
    let profile = serde_json::json!({
        "file":"selected.rs", "module":["archive"], "typeName":"Message",
        "variant":"Chunk", "encode":"pack", "decode":"unpack",
    });
    fs::write(&selection, serde_json::to_vec(&profile).unwrap()).unwrap();
    let fixture = include_str!("fixtures/tagged-wire/selected.rs.txt");
    fs::write(&source, fixture).unwrap();
    let facts = fs::read_to_string(GRAPH)
        .unwrap()
        .replace("t:tag 42 ; ", "")
        .replace("\"index\"", "\"start\"")
        .replace("\"data\"", "\"content\"")
        .replace(
            "t:width 8 ; t:max 18446744073709551614",
            "t:width 4 ; t:max 4294967294",
        );
    let build = |facts: &str, variant: &str, output: &Path| {
        fs::write(&overlay, facts).unwrap();
        let observed = std::process::Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(["recover", "tagged-contract"])
            .arg(temp.path())
            .arg(&selection)
            .arg(&overlay)
            .arg(variant)
            .arg(output)
            .output()
            .unwrap();
        (
            observed.status.success(),
            String::from_utf8(observed.stderr).unwrap(),
        )
    };
    let variant = "urn:example:packets:Chunk";
    let (ok, message) = build(&facts, variant, &out);
    assert!(ok, "{message}");
    let graph = load(&out).unwrap();
    assert_eq!(graph.variant_name(19), Some("Chunk"));
    fs::write(&source, fixture.replace("19", "27")).unwrap();
    let changed = temp.path().join("changed.ttl");
    let (ok, message) = build(&facts, variant, &changed);
    assert!(ok, "{message}");
    assert_eq!(load(&changed).unwrap().variant_name(27), Some("Chunk"));
    assert_ne!(load(&changed).unwrap().sha256(), graph.sha256());

    let rejected = temp.path().join("rejected.ttl");
    for (facts, variant, expected) in [
        (
            facts.replace("t:width 4", "t:width 8"),
            variant,
            "field 0 differs",
        ),
        (
            format!("{facts}\ne:Chunk t:tag 19 .\n"),
            variant,
            "already supplies",
        ),
        (
            facts.clone(),
            "urn:example:packets:Ping",
            "already supplies",
        ),
        (
            facts.replace("\"Chunk\"", "\"Other\""),
            variant,
            "variant name differs",
        ),
    ] {
        let (ok, message) = build(&facts, variant, &rejected);
        assert!(!ok && message.contains(expected), "{message}");
        assert!(!rejected.exists());
    }
}

#[test]
fn generated_rust_module_compiles_and_replays_a_frozen_packet() {
    let contract = load(Path::new(GRAPH)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let generated = temp.path().join("generated");
    let build = std::process::Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["build", "tagged-wire", "rust", GRAPH])
        .arg(&generated)
        .args(["--graph-sha256", contract.sha256()])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let probe = generated.join("probe.rs");
    fs::write(
        &probe,
        r#"
mod wire;
fn main() {
    let expected = [62, 0, 0, 0, 3, 1, 2, 3];
    let values = [wire::Value::Bytes(vec![1, 2, 3])];
    let contract = wire::schema();
    assert_eq!(contract.encode(62, &values).unwrap(), expected);
    assert_eq!(contract.decode(&expected).unwrap(), (62, values.into()));
}
"#,
    )
    .unwrap();
    let executable = generated.join("probe");
    let native = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(&probe)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        std::process::Command::new(executable)
            .status()
            .unwrap()
            .success()
    );
}
