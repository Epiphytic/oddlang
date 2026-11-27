use super::*;
use tempfile::TempDir;
use transcripts::{Outcomes, deltas, improvements, metadata, outcomes, sdk};
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(root().join("examples/marmish/evidence/qualification.tar.gz"))
        .arg("-C")
        .arg(dir.path())
        .arg("native")
        .status()
        .unwrap();
    assert!(status.success());
    dir
}
fn rows(pairs: &[(&str, &str)]) -> Outcomes {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
#[test]
fn declared_nested_tests_are_recovered() {
    let dir = fixture();
    for (name, count) in [("workspace", 920), ("installed", 918)] {
        let base = dir.path().join("native/original/native/logs");
        let rows = outcomes(
            &fs::read_to_string(base.join(format!("{name}.stdout"))).unwrap(),
            &fs::read_to_string(base.join(format!("{name}.stderr"))).unwrap(),
        )
        .unwrap();
        assert_eq!(rows.len(), count);
    }
}
#[test]
fn balanced_swaps_retain_both_differences() {
    assert_eq!(
        json!(deltas(
            &rows(&[("a", "ok"), ("b", "FAILED")]),
            &rows(&[("a", "FAILED"), ("b", "ok")])
        )),
        json!([{"test":"a","original":"ok","candidate":"FAILED"},{"test":"b","original":"FAILED","candidate":"ok"}])
    );
}
#[test]
fn improvements_preserve_cargo_exit_and_previous_test_status() {
    let changes = json!([{"command":"workspace","original":101,"candidate":0},{"command":"marmish-worker-help","original":1,"candidate":0},{"command":"installed","original":0,"candidate":101}]);
    let ds = deltas(
        &rows(&[
            ("fixed", "FAILED"),
            ("activated", "ignored, platform"),
            ("broken", "ok"),
        ]),
        &rows(&[
            ("fixed", "ok"),
            ("activated", "ok"),
            ("broken", "FAILED"),
            ("new", "ok"),
        ]),
    );
    let tests = json!({"workspace":{"differences":ds}});
    let actual = improvements(array(&changes).unwrap(), object(&tests).unwrap()).unwrap();
    assert_eq!(
        json!(actual),
        json!([{"kind":"cargo-exit","command":"workspace","original":101,"candidate":0},{"kind":"test","suite":"workspace","test":"activated","original":"ignored, platform","candidate":"ok"},{"kind":"test","suite":"workspace","test":"fixed","original":"FAILED","candidate":"ok"}])
    );
}
#[test]
fn truncated_log_rejected() {
    let err=outcomes("running 2 tests\ntest good ... ok\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n","     Running tests/demo.rs (/tmp/demo-0123456789abcdef)\n").unwrap_err();
    assert!(err.to_string().contains("individual outcomes"));
}
#[test]
fn sdk_only_normalizes_random_suffix() {
    let warning = "xcrun: error: couldn't create cache file '/var/folders/aa/bb/T/xcrun_db-Ab123456' (errno=Operation not permitted)\n";
    let baseline = sdk(warning.as_bytes());
    assert_eq!(
        baseline,
        sdk(warning.replace("Ab123456", "XY654321").as_bytes())
    );
    for change in [
        warning.replace("not permitted", "not found"),
        warning.replace("/aa/", "/bb/"),
        warning.repeat(2),
    ] {
        assert_ne!(baseline, sdk(change.as_bytes()));
    }
}
#[test]
fn missing_and_new_cases_explicit() {
    let ds = deltas(&rows(&[("old", "ignored")]), &rows(&[("new", "ok")]));
    assert_eq!(
        ds.iter()
            .map(|r| r["original"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["unobserved", "ignored"]
    );
    assert_eq!(
        ds.iter()
            .map(|r| r["candidate"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["ok", "unobserved"]
    );
}
#[test]
fn metadata_normalizes_build_roots_and_retains_features() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("meta");
    let save = |target, features: Value| {
        fs::write(&p, json!({"target":target,"features":features}).to_string()).unwrap()
    };
    save("/private/tmp/m-a123/target", json!(["installed-checks"]));
    let first = metadata(&p).unwrap();
    save("/private/tmp/m-b456/target", json!(["installed-checks"]));
    assert_eq!(metadata(&p).unwrap(), first);
    save("/private/tmp/m-b456/target", json!([]));
    assert_ne!(metadata(&p).unwrap(), first);
}
#[test]
fn archived_evidence_provenance_and_candidate_mutations() {
    let tmp = fixture();
    let dir = tmp.path().join("native");
    let published = read_json(&dir.join("qualification.json")).unwrap();
    let run = || {
        compare(
            &dir,
            PIN,
            text(&published["candidateCommit"]).unwrap(),
            text(&published["candidateTree"]).unwrap(),
            LEGACY_VERIFIER,
        )
    };
    assert_eq!(run().unwrap(), published);
    let receipt = dir.join("original/receipt.json");
    let bytes = fs::read(&receipt).unwrap();
    fs::write(&receipt, b"{}").unwrap();
    assert!(
        run()
            .unwrap_err()
            .to_string()
            .contains("original receipt changed")
    );
    fs::write(&receipt, bytes).unwrap();
    let stdout = dir.join("candidate/logs/marmish-help.stdout");
    let bytes = fs::read(&stdout).unwrap();
    fs::write(&stdout, b"changed").unwrap();
    assert!(
        run()
            .unwrap_err()
            .to_string()
            .contains("native stream digest mismatch")
    );
    fs::write(&stdout, bytes).unwrap();
    fs::write(dir.join("candidate/install-status.stderr"), b"changed").unwrap();
    let regressions = run().unwrap();
    assert_eq!(regressions["status"], "regressed");
    assert_eq!(
        regressions["regressions"],
        json!(["installed layout output differs from expected bytes"])
    );
}
