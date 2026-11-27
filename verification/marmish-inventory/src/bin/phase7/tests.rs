use super::*;
use tempfile::TempDir;

fn archive(parts: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(root().join("examples/marmish/evidence/qualification.tar.gz"))
        .arg("-C")
        .arg(dir.path())
        .args(parts)
        .status()
        .unwrap();
    assert!(status.success());
    dir
}
fn wire_fixture() -> (Value, Value) {
    let dir = archive(&["wire.json"]);
    (
        read_json(&root().join("examples/marmish/observations.json")).unwrap(),
        read_json(&dir.path().join("wire.json")).unwrap(),
    )
}
fn builds() -> (TempDir, Value) {
    let path = "native/original/inventory/inventory.json.gz";
    let dir = archive(&["rebuild", path]);
    let inventory = serde_json::from_reader(GzDecoder::new(
        fs::File::open(dir.path().join(path)).unwrap(),
    ))
    .unwrap();
    (dir, inventory)
}
fn pair(dir: &TempDir, inventory: &Value) -> Result<Value> {
    reconstruction::build_pair(
        &dir.path().join("rebuild"),
        inventory,
        &root().join("examples/marmish"),
    )
}
#[test]
fn changed_wire_byte_rejects_even_a_matching_witness_digest() {
    let (contract, mut wire) = wire_fixture();
    frame::check_wire(&contract, &wire).unwrap();
    let id = wire["directions"][1]["exchanges"][0]["id"].clone();
    let row = wire["cases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["id"] == id)
        .unwrap();
    let mut body = hex_decode(text(&row["observed"]["bytesHex"]).unwrap()).unwrap();
    body[0] ^= 1;
    row["observed"]["bytesHex"] = json!(hex_encode(&body));
    wire["directions"][1]["exchanges"][0]["wireSha256"] = json!(digest(&body));
    assert!(frame::check_wire(&contract, &wire).is_err());
}
#[test]
fn exchanges_require_both_directions_and_exact_ids() {
    let (contract, wire) = wire_fixture();
    let mut altered = wire.clone();
    altered["directions"][0]["writer"] = json!("candidate");
    assert!(frame::check_wire(&contract, &altered).is_err());
    let mut altered = wire;
    altered["directions"][0]["exchanges"][0]["id"] = json!("substituted");
    assert!(frame::check_wire(&contract, &altered).is_err());
}
#[test]
fn deleted_generated_file_invalidates_producer() {
    let (dir, inventory) = builds();
    pair(&dir, &inventory).unwrap();
    fs::remove_file(dir.path().join("rebuild/first/framing-projection/frame.rs")).unwrap();
    assert!(
        pair(&dir, &inventory)
            .unwrap_err()
            .to_string()
            .contains("missing or redirected evidence")
    );
}
#[test]
fn retained_digest_matches_frozen_original() {
    let files = json!({"fixture.dat":{"mode":"100644","sha256":digest(b"original")}});
    let mut row = json!({"path":"fixture.dat","mode":"100644","sha256":digest(b"original"),"producer":"retained"});
    reconstruction::check_source_rows(&[row.clone()], object(&files).unwrap(), &Set::new())
        .unwrap();
    row["sha256"] = json!(digest(b"changed"));
    assert!(
        reconstruction::check_source_rows(&[row], object(&files).unwrap(), &Set::new()).is_err()
    );
}
#[test]
fn omitted_platform_record_rejects() {
    let dir = archive(&["qualification.json"]);
    let mut platforms =
        read_json(&dir.path().join("qualification.json")).unwrap()["platforms"].clone();
    check_platforms(&platforms).unwrap();
    platforms
        .as_object_mut()
        .unwrap()
        .remove("aarch64-unknown-linux-gnu");
    assert!(check_platforms(&platforms).is_err());
}
#[test]
fn native_exit_three_rejects_even_a_passing_summary() {
    let passing = json!({"nativeVerdict":"passed measured observations","newRegressions":[]});
    passed(0, &passing).unwrap();
    assert!(
        passed(3, &passing)
            .unwrap_err()
            .to_string()
            .contains("native candidate regression")
    );
}
#[test]
fn retained_sites_require_workspace_package() {
    let inventory = json!({"cargo":{"crates":{}},"symbols":[{"classification":"retained-native","path":"other"}]});
    assert!(
        coverage::coverage(&inventory, &Set::new(), &Value::Null)
            .unwrap_err()
            .to_string()
            .contains("retained path has no workspace package: other")
    );
}
#[test]
fn second_build_must_supply_its_own_artifacts() {
    let (dir, inventory) = builds();
    pair(&dir, &inventory).unwrap();
    fs::remove_file(dir.path().join("rebuild/second/facade-projection/lib.rs")).unwrap();
    assert!(
        pair(&dir, &inventory)
            .unwrap_err()
            .to_string()
            .contains("second/facade-projection/lib.rs")
    );
}
#[test]
fn coherent_tree_and_commit_rewrites_reject() {
    for mutation in [
        "claimed commit",
        "claimed tree",
        "modified commit object",
        "modified ontology blob",
    ] {
        let (dir, inventory) = builds();
        let base = dir.path().join("rebuild");
        let key = if mutation == "claimed tree" {
            "candidateTree"
        } else {
            "candidateCommit"
        };
        let mut oid = "f".repeat(40);
        for run in ["first", "second"] {
            if mutation == "modified commit object" {
                let path = base.join(run).join("immutable/commit.raw");
                let raw = fs::read_to_string(&path)
                    .unwrap()
                    .replace("Qualify regenerated", "Unexpected regenerated");
                fs::write(&path, &raw).unwrap();
                oid = reconstruction::git_object("commit", raw.as_bytes());
            }
            if mutation == "modified ontology blob" {
                fs::write(
                    base.join(run).join("immutable/symbols.jsonl"),
                    b"other ontology",
                )
                .unwrap();
            }
        }
        if mutation != "modified ontology blob" {
            for name in ["first/result.json", "second/result.json", "rebuild.json"] {
                let path = base.join(name);
                let mut doc = read_json(&path).unwrap();
                doc[key] = json!(oid);
                fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
            }
        }
        let err = pair(&dir, &inventory).unwrap_err().to_string();
        assert!(
            err.contains("candidate Git tree") || err.contains("candidate Git commit"),
            "{mutation}: {err}"
        );
    }
}
