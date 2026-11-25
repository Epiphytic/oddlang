use oddlang::http::{cases, gaps, inventory, load_source, publish};
use std::path::Path;
fn model() -> oddlang::http::HttpContract {
    let root = Path::new("examples/http/contract/v1");
    load_source(
        root,
        &root.join("dependencies.lock.json"),
        Path::new("contracts/cache"),
    )
    .unwrap()
}
#[test]
fn publication_requires_all_witnesses() {
    let model = model();
    let all = cases(&model).unwrap();
    let required = inventory(&model);
    assert!(gaps(&required, &all).is_empty());
    for kind in ["present", "omitted"] {
        let mut subset = all.clone();
        let index = subset
            .iter()
            .position(|case| case.slots.iter().any(|s| s.kind == kind))
            .unwrap();
        let slot = subset[index]
            .slots
            .iter()
            .find(|s| s.kind == kind)
            .unwrap()
            .clone();
        subset.retain(|case| !case.slots.contains(&slot));
        assert!(!gaps(&required, &subset).is_empty());
        assert!(
            oddlang::http::validate_coverage(&required, &subset)
                .unwrap_err()
                .contains("uncovered witness")
        );
    }
    publish(&model, tempfile::tempdir().unwrap().path()).unwrap();
}

use base64::{Engine, engine::general_purpose::STANDARD};
use oddlang::http::{
    Case, CaseResult, Claim, ClaimPair, Evidence, select_cases, verify_bundle, verify_evidence,
};
use serde_json::{Value, json};
use std::fs;
#[path = "support/ledger_oracle.rs"]
mod oracle;
fn body(raw: &Value) -> Value {
    serde_json::from_slice(
        &STANDARD
            .decode(raw["bodyBase64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn independent_ledger_outcomes_and_full_poststates() {
    let all = cases(&model()).unwrap();
    let mut count = 0;
    for c in all
        .iter()
        .filter(|c| c.role == "http.provider" && c.case_id.starts_with("scenario/TransferFunds/"))
    {
        count += 1;
        let mut state = oracle::State::new();
        for a in c.fixture.as_array().unwrap() {
            let id = a["id"].as_str().unwrap().to_owned();
            state.insert(
                id.clone(),
                oracle::Account {
                    id,
                    owner: a["owner"].as_str().unwrap().into(),
                    balance: a["balance"].as_str().unwrap().parse().unwrap(),
                },
            );
        }
        let input = body(&c.action["requests"][0]);
        let result = oracle::transfer(
            &mut state,
            input["source"].as_str().unwrap(),
            input["destination"].as_str().unwrap(),
            input["amount"].as_str().unwrap().parse().unwrap(),
        );
        let response = &c.expected[0]["response"];
        let actual = body(response);
        match result {
            Err(err) => assert_eq!(actual, json!({"error":err})),
            Ok(()) => {
                assert_eq!(response["status"], 200);
                for key in ["source", "destination"] {
                    assert_eq!(
                        actual[key]["balance"],
                        state[input[key].as_str().unwrap()].balance.to_string()
                    )
                }
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for exchange in &c.expected[1..] {
            let a = body(&exchange["response"]);
            let id = a["id"].as_str().unwrap();
            assert!(seen.insert(id.to_owned()));
            assert_eq!(exchange["request"]["path"], format!("/accounts/{id}"));
            assert_eq!(exchange["response"]["status"], 200);
            assert_eq!(
                a,
                json!({"id":id,"balance":state[id].balance.to_string(),"owner":state[id].owner})
            );
        }
        assert_eq!(seen.len(), state.len());
    }
    assert_eq!(count, 12);
    assert!(all.iter().any(|c| c.role == "http.provider"
        && c.operation.ends_with("LookupAccount")
        && c.expected[0]["response"]["status"] == 200
        && body(&c.expected[0]["response"])["balance"] == "9223372036854775807"));
    assert!(all.iter().any(|c| {
        c.role == "http.provider"
            && c.slots
                .iter()
                .any(|s| s.position == "request/amount" && s.kind == "overflow")
            && body(&c.action["requests"][0])["amount"] == "9223372036854775808"
            && c.expected[0]["response"]["status"] == 400
    }));
}

#[test]
fn evidence_rejects_fake_success_missing_duplicates_and_stale_pin() {
    let out = tempfile::tempdir().unwrap();
    publish(&model(), out.path()).unwrap();
    let bundle = verify_bundle(out.path(), &out.path().join("workspace.lock.json")).unwrap();
    let claim = Claim {
        schema_version: 1,
        claims: vec![ClaimPair {
            operation: "urn:oddlang:ledger-http:LookupAccount".into(),
            role: "http.consumer".into(),
        }],
    };
    let ids = select_cases(&bundle, &claim).unwrap();
    assert!(!ids.is_empty());
    let mut e = Evidence {
        schema_version: 1,
        release: bundle.pin.clone(),
        rule_set_version: 1,
        claims: claim.claims.clone(),
        results: bundle
            .cases
            .iter()
            .filter(|c| ids.contains(&c.case_id))
            .map(|c| CaseResult {
                case_id: c.case_id.clone(),
                outcome: "pass".into(),
                observations: c.expected.clone(),
            })
            .collect(),
    };
    verify_evidence(&bundle, &claim, &e).unwrap();
    for action in 0..6 {
        let mut bad = e.clone();
        match action {
            0 => {
                bad.results.remove(0);
            }
            1 => bad.results.push(bad.results[0].clone()),
            2 => {
                bad.results[0].observations.clear();
            }
            3 => {
                let more = bad.results[0].observations[0].clone();
                bad.results[0].observations.push(more);
            }
            4 => {
                bad.results[0].observations[0] = json!({"incorrect":true});
            }
            _ => {
                bad.release.version = "stale".into();
            }
        };
        assert!(verify_evidence(&bundle, &claim, &bad).is_err());
    }
    e.results[0].outcome = "fail".into();
    assert!(verify_evidence(&bundle, &claim, &e).is_err());
}
#[test]
fn releases_reproduce_and_semantic_rewrites_preserve_projections() {
    let source = model();
    let releases = (0..3)
        .map(|_| tempfile::tempdir().unwrap())
        .collect::<Vec<_>>();
    publish(&source, releases[0].path()).unwrap();
    publish(&source, releases[1].path()).unwrap();
    let root = tempfile::tempdir().unwrap();
    for (path, bytes) in &source.source_files {
        fs::write(root.path().join(path), bytes).unwrap()
    }
    let domain = String::from_utf8(source.source_files["domain.ttl"].clone()).unwrap();
    fs::write(
        root.path().join("domain.ttl"),
        format!(
            "{domain}\n_:unused <http://www.w3.org/2000/01/rdf-schema#comment> \"annotation\" .\n"
        ),
    )
    .unwrap();
    let equivalent = load_source(
        root.path(),
        Path::new("examples/http/contract/v1/dependencies.lock.json"),
        Path::new("contracts/cache"),
    )
    .unwrap();
    publish(&equivalent, releases[2].path()).unwrap();
    for entry in fs::read_dir(releases[0].path()).unwrap() {
        let file = entry.unwrap().file_name();
        let bytes = fs::read(releases[0].path().join(&file)).unwrap();
        assert_eq!(bytes, fs::read(releases[1].path().join(&file)).unwrap());
        let changed = fs::read(releases[2].path().join(&file)).unwrap();
        if matches!(
            file.to_str().unwrap(),
            "domain.ttl" | "manifest.json" | "workspace.lock.json"
        ) {
            assert_ne!(bytes, changed)
        } else {
            assert_eq!(bytes, changed)
        }
    }
    let mut invalid = source;
    let mut authored: Vec<Case> =
        serde_json::from_slice(&invalid.source_files["scenarios.json"]).unwrap();
    authored[0].slots[0].role = "http.consumer".into();
    invalid.source_files.insert(
        "scenarios.json".into(),
        serde_json::to_vec(&authored).unwrap(),
    );
    assert!(
        publish(&invalid, tempfile::tempdir().unwrap().path())
            .unwrap_err()
            .contains("invalid scenario slots")
    );
}
#[test]
fn expected_inventory_includes_every_v1_position() {
    let mut actual = inventory(&model())
        .into_iter()
        .map(|o| {
            let prefix = "urn:oddlang:ledger-http:";
            let id = o
                .declaration
                .strip_prefix(prefix)
                .unwrap_or_else(|| o.declaration.strip_prefix("urn:oddlang:").unwrap());
            let mut kinds = o.slots;
            kinds.sort();
            [
                o.operation.strip_prefix(prefix).unwrap(),
                o.role.strip_prefix("http.").unwrap(),
                &o.position,
                id,
                o.rule.strip_prefix("urn:oddlang:http:rule:1:").unwrap(),
                &kinds.join(" "),
            ]
            .join("\t")
        })
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(
        actual.join("\n") + "\n",
        include_str!("fixtures/http-v1-inventory.tsv")
    );
}
#[test]
fn pinned_standards_validators_accept_projections() {
    let out = tempfile::tempdir().unwrap();
    publish(&model(), out.path()).unwrap();
    let run = std::process::Command::new("node")
        .arg("tests/support/http-schema.cjs")
        .arg(out.path())
        .output()
        .expect("node is required: run scripts/setup-http-conformance.sh");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
#[test]
fn normalization_requires_observed_wire_bytes_and_segment_boundaries() {
    use oddlang::http::compare_observations;
    let sample = vec![
        json!({"request":{"method":"GET","path":"/accounts/A%2fb","contentType":null,"bodyBase64":""},"response":{"status":200,"contentType":"application/json","bodyBase64":"e30="}}),
    ];
    let mut different = sample.clone();
    different[0]["request"]["path"] = json!("/accounts/A/b");
    assert!(!compare_observations(&sample, &different));
    different[0]["request"]["path"] = json!("/accounts/A%2Fb");
    different[0]["response"]["contentType"] = json!("Application/JSON; Charset = \"UTF-8\"");
    assert!(compare_observations(&sample, &different));
    different[0]["response"]["bodyBase64"] = json!([{}]);
    assert!(!compare_observations(&sample, &different));
    different = sample.clone();
    different[0]["request"]["path"] = json!(["", "accounts", "A/b"]);
    assert!(!compare_observations(&sample, &different));
}

#[test]
fn rebuilt_release_matches_workspace_pin_and_cached_payloads() {
    let out = tempfile::tempdir().unwrap();
    publish(&model(), out.path()).unwrap();
    let lock = Path::new("examples/http/contract/v1/workspace.lock.json");
    let expected = verify_bundle(out.path(), lock).unwrap();
    let cache = Path::new(
        "contracts/cache/http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4/v1",
    );
    let cached = verify_bundle(cache, lock).unwrap();
    assert_eq!(expected.pin, cached.pin);
    assert_eq!(
        fs::read(lock).unwrap(),
        fs::read(out.path().join("workspace.lock.json")).unwrap()
    );
    for f in fs::read_dir(cache).unwrap() {
        let file = f.unwrap();
        assert_eq!(
            fs::read(file.path()).unwrap(),
            fs::read(out.path().join(file.file_name())).unwrap()
        );
    }
}

#[test]
fn rejected_path_decimal_and_media_witnesses() {
    let all = cases(&model()).unwrap();
    for (kind, path) in [
        ("empty", "/accounts/"),
        ("dot", "/accounts/%2E"),
        ("dot-dot", "/accounts/%2E%2E"),
    ] {
        let c = all
            .iter()
            .find(|c| {
                c.role == "http.provider"
                    && c.slots
                        .iter()
                        .any(|s| s.rule.ends_with(":route") && s.kind == kind)
            })
            .expect("provider rejection witness");
        assert_eq!(c.action["requests"][0]["path"], path);
        assert_eq!(c.expected[0]["response"]["status"], 400);
        assert_eq!(
            body(&c.expected[0]["response"]),
            json!({"error":"urn:oddlang:http:error:InvalidRequest"})
        );
    }
    for (kind, lexeme) in [
        ("negative-zero", "-0"),
        ("leading-plus", "+1"),
        ("exponent", "1e3"),
        ("whitespace", " 1"),
    ] {
        let c = all
            .iter()
            .find(|c| {
                c.role == "http.provider"
                    && c.slots
                        .iter()
                        .any(|s| s.position == "request/amount" && s.kind == kind)
            })
            .expect("decimal witness");
        assert_eq!(body(&c.action["requests"][0])["amount"], lexeme);
        assert_eq!(c.expected[0]["response"]["status"], 400);
    }
    for kind in ["wrong-charset", "extra-parameter", "media-before-body"] {
        let c = all
            .iter()
            .find(|c| c.role == "http.provider" && c.slots.iter().any(|s| s.kind == kind))
            .expect("media witness");
        assert_eq!(c.expected[0]["response"]["status"], 415);
        if kind == "media-before-body" {
            assert_eq!(
                STANDARD
                    .decode(c.action["requests"][0]["bodyBase64"].as_str().unwrap())
                    .unwrap(),
                b"{"
            );
        }
    }
}
