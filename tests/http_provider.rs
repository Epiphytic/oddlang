use oddlang::http::load_overlay;
use std::path::Path;

#[test]
fn required_provider_binding() {
    load_overlay(Path::new("examples/http/provider/overlay.json")).unwrap();
}

#[path = "support/http_provider.rs"]
mod live;
use oddlang::http::{
    Case, CaseResult, Claim, Evidence, compare_observations, read_json, verify_bundle,
    verify_evidence,
};
use serde_json::json;
use std::fs;

fn run(binary: &Path, case: &Case) -> CaseResult {
    let provider = live::Provider::launch(binary, &case.fixture);
    let observations: Vec<_> = case.action["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| provider.exchange(r))
        .collect();
    let ok = compare_observations(&case.expected, &observations);
    CaseResult {
        case_id: case.case_id.clone(),
        outcome: if ok { "pass" } else { "fail" }.into(),
        observations,
    }
}

#[test]
fn provider_conformance_and_mutations() {
    let project = Path::new("examples/http/contract/v1");
    let pin = project.join("workspace.lock.json");
    let cache = Path::new("contracts/cache");
    let example = Path::new("examples/http/provider");
    let build = live::ProviderBuild::generate(
        &project.join("project.ttl"),
        &pin,
        cache,
        &example.join("overlay.json"),
    );
    let generated = build.root.path().join("generated");
    for file in ["binding.rs", "ledger.rs"] {
        assert_eq!(
            fs::read(generated.join(file)).unwrap(),
            fs::read(example.join("generated").join(file)).unwrap()
        );
    }
    let binary = build.build();
    let mut oversized = vec![b' '; 1024 * 1024];
    oversized.extend(br#"{"source":"A","destination":"B","amount":"1"}"#);
    let service = live::Provider::launch(
        &binary,
        &json!([{"id":"A","owner":"a","balance":"10"},{"id":"B","owner":"b","balance":"7"}]),
    );
    let response = service.exchange(&json!({
        "method":"POST",
        "path":"/transfers",
        "contentType":"application/json",
        "bodyBase64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD, oversized)
    }));
    assert_eq!(response["response"]["status"], 400);
    let bundle = verify_bundle(
        &cache.join("http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4/v1"),
        &pin,
    )
    .unwrap();
    let claim: Claim = read_json(&example.join("claim.json")).unwrap();
    let stock: Vec<_> = bundle
        .cases
        .iter()
        .filter(|c| c.role == "http.provider")
        .map(|c| run(&binary, c))
        .collect();
    assert_eq!(stock.len(), 117);
    let evidence = Evidence {
        schema_version: 1,
        release: bundle.pin.clone(),
        rule_set_version: bundle.rule_set_version,
        claims: claim.claims.clone(),
        results: stock,
    };
    verify_evidence(&bundle, &claim, &evidence).unwrap();
    let path = Path::new("target/http-conformance/provider-v1.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    let ledger = fs::read_to_string(generated.join("ledger.rs")).unwrap();
    let source = ledger
        .lines()
        .find(|s| s.contains("return Err(\"urn:oddlang:ledger:error:SourceMissing"))
        .unwrap();
    let dest = ledger
        .lines()
        .find(|s| s.contains("return Err(\"urn:oddlang:ledger:error:DestinationMissing"))
        .unwrap();
    let binding = fs::read_to_string(generated.join("binding.rs")).unwrap();
    let write_on_error = ledger.replace(
        "return Err(\"urn:oddlang:ledger:error:InsufficientFunds",
        concat!(
            "state.0.get_mut(source).unwrap().balance = super::Balance::try_from(0).unwrap(); ",
            "return Err(\"urn:oddlang:ledger:error:InsufficientFunds"
        ),
    );
    let swapped = ledger.replace(&format!("{source}\n{dest}"), &format!("{dest}\n{source}"));
    let permissive = binding.replace(
        "if value.to_string() != text",
        "if false && value.to_string() != text",
    );
    for (file, mutant, case_suffix, step) in [
        ("ledger.rs", write_on_error, "/insufficient", 1),
        ("ledger.rs", swapped, "/missing-both", 0),
        ("binding.rs", permissive, "/noncanonical", 0),
    ] {
        fs::write(generated.join(file), mutant).unwrap();
        let binary = build.build();
        let case = bundle
            .cases
            .iter()
            .find(|c| c.role == "http.provider" && c.case_id.ends_with(case_suffix))
            .unwrap();
        let result = run(&binary, case);
        assert_eq!(result.outcome, "fail", "{}", case.case_id);
        assert!(
            !compare_observations(
                &case.expected[step..=step],
                &result.observations[step..=step]
            ),
            "{}",
            case.case_id
        );
        assert!(compare_observations(
            &case.expected[..step],
            &result.observations[..step]
        ));
        fs::write(
            generated.join(file),
            if file == "ledger.rs" {
                &ledger
            } else {
                &binding
            },
        )
        .unwrap();
    }
}
#[test]
fn overlay_validation_and_advisory_hints() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("overlay.json");
    let binding = json!({"role":"http.provider","target":"rust","binding":"axum"});
    for bindings in [
        json!([]),
        json!([{"role":"http.provider","target":"rust","binding":"unknown"}]),
    ] {
        fs::write(
            &path,
            json!({"schemaVersion":1,"bindings":bindings}).to_string(),
        )
        .unwrap();
        assert!(load_overlay(&path).is_err());
    }
    let project = Path::new("examples/http/contract/v1");
    for hint in ["library A", "library B"] {
        fs::write(&path,json!({"schemaVersion":1,"bindings":[binding],"hints":[{"role":"http.provider","target":"rust","hint":hint}]}).to_string()).unwrap();
        let build = live::ProviderBuild::generate(
            &project.join("project.ttl"),
            &project.join("workspace.lock.json"),
            Path::new("contracts/cache"),
            &path,
        );
        assert_eq!(
            fs::read(build.root.path().join("generated/binding.rs")).unwrap(),
            fs::read("examples/http/provider/generated/binding.rs").unwrap()
        );
    }
}
