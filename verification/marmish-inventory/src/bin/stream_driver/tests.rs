use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::GzDecoder;
use std::io::Read;

fn historical(name: &str) -> Value {
    let base = root().join("examples/marmish/evidence");
    if name == "original" || name == "mutation" {
        let file = if name == "original" {
            "stream-baseline/oracle-original.json.gz"
        } else {
            "stream-qualification/oracle-mutation.json.gz"
        };
        return serde_json::from_reader(GzDecoder::new(fs::File::open(base.join(file)).unwrap()))
            .unwrap();
    }
    let tar = fs::File::open(base.join("stream-qualification/qualification.tar.gz")).unwrap();
    let mut archive = tar::Archive::new(GzDecoder::new(tar));
    let target = format!("{name}-oracle.json");
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().ends_with(&target) {
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            return serde_json::from_slice(&data).unwrap();
        }
    }
    panic!("missing historical receipt {name}")
}
fn corpus() -> Value {
    read_json(&root().join("examples/marmish/stream-vectors.json")).unwrap()
}
#[test]
fn original_mutant_and_both_mixed_receipts_replay() {
    let corpus = corpus();
    let cases = array(&corpus["cases"]).unwrap();
    let original = historical("original");
    let candidate = historical("candidate");
    let reverse = historical("reverse");
    for receipt in [&original, &candidate, &reverse, &historical("mutation")] {
        let rows = array(&receipt["observations"]).unwrap();
        assert_eq!(rows.len(), 50);
        let mut failed = vec![];
        for (row, case) in rows.iter().zip(cases) {
            assert_eq!(row["name"], case["name"]);
            let valid = matches(&row["actual"], &expand(&case["expected"]).unwrap());
            assert_eq!(row["matched"], valid);
            if !valid {
                failed.push(&row["name"]);
            }
        }
        if receipt["role"] == "mutation" {
            assert_eq!(
                json!(failed),
                json!(["binary-data-encode", "maximum-data-encode"])
            );
        } else {
            assert!(failed.is_empty());
        }
        assert_eq!(
            receipt["driver_binding_sha256"],
            binding(
                &json!({"workspace":receipt["workspace_identity"],"libraries":receipt["libraries"]})
            )
        );
    }
    for (local, peer) in [(&candidate, &original), (&reverse, &candidate)] {
        let derived = peer_cases(
            peer,
            cases,
            text(&local["vectors_sha256"]).unwrap(),
            text(&local["driver_source_sha256"]).unwrap(),
            &local["workspace_identity"],
            binding,
        )
        .unwrap();
        let observations = array(&local["mixed_observations"]).unwrap();
        assert_eq!(derived.len(), 9);
        assert_eq!(observations.len(), 9);
        for (case, row) in derived.iter().zip(observations) {
            assert_eq!(case["name"], row["name"]);
            assert!(matches(&row["actual"], &expand(&case["expected"]).unwrap()));
        }
    }
}
#[test]
fn peer_input_must_be_authentic() {
    let corpus = corpus();
    let cases = array(&corpus["cases"]).unwrap();
    let original = historical("original");
    let candidate = historical("candidate");
    for field in [
        "role",
        "vectors",
        "library",
        "binding",
        "workspace",
        "case",
        "order",
        "verdict",
    ] {
        let mut peer = original.clone();
        match field {
            "role" => peer["role"] = json!("candidate"),
            "vectors" => peer["vectors_sha256"] = json!("changed"),
            "library" => peer["libraries"]["marmish_stream"]["sha256"] = json!("changed"),
            "binding" => peer["driver_binding_sha256"] = json!("changed"),
            "workspace" => {
                peer["workspace_identity"]["root"] = candidate["workspace_identity"]["root"].clone()
            }
            "case" => peer["observations"][0]["actual"] = json!(false),
            "order" => peer["observations"].as_array_mut().unwrap().swap(0, 1),
            "verdict" => peer["all_matched"] = json!(false),
            _ => unreachable!(),
        }
        assert!(
            peer_cases(
                &peer,
                cases,
                text(&candidate["vectors_sha256"]).unwrap(),
                text(&candidate["driver_source_sha256"]).unwrap(),
                &candidate["workspace_identity"],
                binding
            )
            .is_err(),
            "{field}"
        );
    }
}
#[test]
fn zlib_comparison_requires_complete_frame_and_exact_bytes() {
    let original = historical("original");
    let c = corpus();
    let case = array(&c["cases"])
        .unwrap()
        .iter()
        .find(|r| r["name"] == "codec-zlib-encode")
        .unwrap();
    let expected = expand(&case["expected"]).unwrap();
    let actual = &array(&original["observations"])
        .unwrap()
        .iter()
        .find(|r| r["name"] == "codec-zlib-encode")
        .unwrap()["actual"];
    assert!(matches(actual, &expected));
    let encoded = STANDARD.decode(text(&actual["data"]).unwrap()).unwrap();
    let mut trailing = encoded.clone();
    trailing.push(0);
    let mut truncated = encoded.clone();
    truncated.pop();
    for wire in [
        STANDARD.encode(trailing),
        STANDARD.encode(truncated),
        "bad!".to_owned(),
    ] {
        let mut altered = actual.clone();
        altered["data"] = json!(wire);
        assert!(!matches(&altered, &expected));
    }
    let mut altered = actual.clone();
    altered["raw_len"] = json!(0);
    assert!(!matches(&altered, &expected));
    assert!(!matches(&json!(true), &json!(1)));
}
