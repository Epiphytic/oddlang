use serde_json::json;
use std::{fs, path::Path, process::Command};

const ID: &str = "urn:sensor:sample";
const OTHER: &str = "urn:sensor:wide-sample";
const FAILURE: &str = "urn:sensor:width";

fn graph(width: u32) -> serde_json::Value {
    json!({
        "format": "oddlang-checked-values-v1",
        "id": "urn:sensor:domain",
        "types": {
            ID: {"kind": "fixed-bytes", "length": width, "failure": FAILURE, "onDrop": "erase"}
        }
    })
}

fn project(graph: &Path, output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["build", "checked-values", "typescript"])
        .arg(graph)
        .arg(output)
        .output()
        .unwrap()
}

#[test]
fn projects_exact_native_bytes_with_independent_input_and_output_copies() {
    let work = tempfile::tempdir().unwrap();
    for width in [0, 7, 9] {
        let input = work.path().join(format!("input-{width}.json"));
        let mut domain = graph(width);
        domain["types"][OTHER] = graph(width + 1)["types"][ID].clone();
        fs::write(&input, serde_json::to_vec(&domain).unwrap()).unwrap();
        let output = work.path().join(format!("out-{width}"));
        let result = project(&input, &output);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );

        let obligations: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("obligations.json")).unwrap()).unwrap();
        for (index, (id, width)) in [(ID, width), (OTHER, width + 1)].into_iter().enumerate() {
            let witness = Command::new("node")
                .args([
                    "--experimental-strip-types",
                    "verification/checked-values/typescript.mjs",
                ])
                .arg(output.join("values.mts"))
                .args([id, &width.to_string(), FAILURE])
                .output()
                .unwrap();
            assert!(
                witness.status.success(),
                "{}",
                String::from_utf8_lossy(&witness.stderr)
            );
            let entry = &obligations["obligations"][index];
            assert_eq!(entry["declaration"], id);
            assert_eq!(entry["expected"]["length"], width);
            assert_eq!(entry["expected"]["failure"], FAILURE);
            assert_eq!(entry["expected"]["onDrop"], "erase");
            assert_eq!(entry["disposal"]["qualification"], "unsupported");
        }
    }
}

#[test]
fn equivalent_graph_bytes_produce_identical_artifacts_and_other_kinds_are_rejected() {
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("graph.json");
    let first = work.path().join("first");
    let second = work.path().join("second");
    let unrelated = work.path().join("unrelated");
    fs::write(
        &input,
        format!(r#"{{"types":{{"{ID}":{{"onDrop":"erase","failure":"{FAILURE}","length":7,"kind":"fixed-bytes"}}}},"id":"urn:sensor:domain","format":"oddlang-checked-values-v1"}}"#),
    ).unwrap();
    let baseline = project(&input, &first);
    assert!(
        baseline.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    fs::write(&input, serde_json::to_vec_pretty(&graph(7)).unwrap()).unwrap();
    assert!(project(&input, &second).status.success());
    for file in ["values.mts", "obligations.json", "projection.lock"] {
        assert_eq!(
            fs::read(first.join(file)).unwrap(),
            fs::read(second.join(file)).unwrap()
        );
    }
    let graph = json!({
        "format": "oddlang-checked-values-v1", "id": "urn:sensor:domain",
        "types": { ID: { "kind": "enum", "values": { "urn:sensor:option": "option" } } }
    });
    fs::write(&input, serde_json::to_vec(&graph).unwrap()).unwrap();
    let rejected = project(&input, &unrelated);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("fixed-byte"));
    assert!(!unrelated.exists());
}
