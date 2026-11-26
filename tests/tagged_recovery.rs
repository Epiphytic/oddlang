use oddlang::rust_recovery::tagged::{TaggedSelection, recover};
use std::{fs, path::PathBuf};

const SOURCE: &str = include_str!("fixtures/tagged-wire/selected.rs.txt");

fn candidate(source: &str) -> Result<serde_json::Value, String> {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("messages.rs"), source).unwrap();
    let selection = TaggedSelection {
        file: PathBuf::from("messages.rs"),
        module: vec!["archive".into()],
        type_name: "Message".into(),
        variant: "Chunk".into(),
        encode: "pack".into(),
        decode: "unpack".into(),
    };
    recover(root.path(), &selection).map(|report| serde_json::to_value(report).unwrap())
}

#[test]
fn source_changes_propagate_to_observed_wire_facts() {
    let before = candidate(SOURCE).unwrap();
    assert_eq!(before["encodedTag"]["value"], 19);
    assert_eq!(before["decodedTag"]["value"], 19);
    assert_eq!(before["fields"][0]["name"], "start");
    assert_eq!(before["fields"][0]["width"], 4);
    assert_eq!(before["fields"][1]["name"], "content");
    assert_eq!(before["fields"][1]["kind"], "rest-bytes");

    let retagged = candidate(&SOURCE.replace("19", "27")).unwrap();
    assert_eq!(retagged["encodedTag"]["value"], 27);
    assert_eq!(retagged["decodedTag"]["value"], 27);
    assert_ne!(before["sourceSha256"], retagged["sourceSha256"]);

    let renamed = SOURCE
        .replace("append", "write_number")
        .replace("pos", "other_pos");
    let renamed = candidate(&renamed).unwrap();
    assert_eq!(renamed["fields"][0]["name"], "start");
    assert_eq!(renamed["fields"][0]["width"], 4);
}

#[test]
fn different_encode_and_decode_tags_are_rejected() {
    let error = candidate(&SOURCE.replace("19 =>", "27 =>")).unwrap_err();
    assert!(
        error.contains("encoder and decoder tags disagree"),
        "{error}"
    );
}

#[test]
fn unrecognized_payload_write_is_rejected() {
    let changed = SOURCE.replace("out.extend(n.to_be_bytes())", "out.extend(n.to_le_bytes())");
    let error = candidate(&changed).unwrap_err();
    assert!(error.contains("unrecognized payload write"), "{error}");
}

#[test]
fn unused_decode_dispatch_is_rejected() {
    let changed = SOURCE.replace(
        "Ok(output)",
        "Ok(Self::Chunk { start: 0, content: Vec::new() })",
    );
    let error = candidate(&changed).unwrap_err();
    assert!(
        error.contains("decoder does not return selected dispatch"),
        "{error}"
    );
}

#[test]
fn a_preceding_encoder_arm_must_be_provably_disjoint() {
    let with_other = SOURCE
        .replace("enum Message {", "enum Message { Other,")
        .replace("impl Message {", "type Alias = Message; impl Message {")
        .replace(
            "                }\n            };",
            "                }\n                Self::Other => 7,\n            };",
        );
    for pattern in [
        "Self::Chunk { .. } | Self::Other",
        "whole @ Self::Chunk { .. }",
        "Message::Chunk { .. }",
        "Alias::Chunk { .. }",
        "_",
    ] {
        let changed = with_other.replace(
            "Self::Chunk { start: pos",
            &format!("{pattern} => 3, Self::Chunk {{ start: pos"),
        );
        let error = candidate(&changed).unwrap_err();
        assert!(
            error.contains("preceding encoder arm needs disjointness evidence"),
            "{pattern}: {error}"
        );
    }
}

#[test]
fn decoder_must_dispatch_on_original_input() {
    for binding in [
        "let input = &input[4..];",
        "let input: &[u8] = &input[4..];",
        "input = &input[4..];",
    ] {
        let changed = SOURCE
            .replace("fn unpack(input:", "fn unpack(mut input:")
            .replace(
                "let output = match input[0]",
                &format!("{binding} let output = match input[0]"),
            );
        let error = candidate(&changed).unwrap_err();
        assert!(
            error.contains("decoder dispatch input lacks provenance"),
            "{binding}: {error}"
        );
    }
}

#[test]
fn decoder_payload_binding_cannot_shadow_input() {
    let changed = SOURCE
        .replace("let payload = &input[5..];", "let input = &input[5..];")
        .replace("payload.split_at_checked", "input.split_at_checked");
    let error = candidate(&changed).unwrap_err();
    assert!(
        error.contains("decoder dispatch input lacks provenance"),
        "{error}"
    );
}

#[test]
fn selected_decoder_variant_cannot_appear_in_another_path() {
    let second_constructor = "Self::Chunk { start: 0, content: Vec::new() }";
    for altered in [
        SOURCE.replace(
            "_ => return Err(\"tag\"),",
            &format!("27 => {{ let other = {second_constructor}; other }}, _ => return Err(\"tag\"),"),
        ),
        SOURCE.replace(
            "_ => return Err(\"tag\"),",
            &format!("27 => if input.len() > 5 {{ {second_constructor} }} else {{ return Err(\"tag\"); }}, _ => return Err(\"tag\"),"),
        ),
        SOURCE.replace(
            "Ok(output)",
            &format!("if input[0] == 27 {{ return Ok({second_constructor}); }} Ok(output)"),
        ),
    ] {
        let error = candidate(&altered).unwrap_err();
        assert!(error.contains("variant also occurs outside its dispatch arm"), "{error}");
    }
}
