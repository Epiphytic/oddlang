use oddlang::http::*;
use std::path::Path;

fn source(version: &str) -> HttpContract {
    let root = Path::new("examples/http/contract").join(version);
    load_source(
        &root,
        &root.join("dependencies.lock.json"),
        Path::new("contracts/cache"),
    )
    .unwrap()
}
#[test]
fn directions_and_stable_declarations() {
    let a = source("v1");
    let b = source("v2");
    let report = compatibility(&a, &b);
    assert_eq!(report.old_client_new_provider, Verdict::Incompatible);
    assert_eq!(report.new_client_old_provider, Verdict::Incompatible);
    assert_eq!(report.changes.len(), 1);
    assert_eq!(
        report.changes[0].declaration,
        "urn:oddlang:ledger-http:balance"
    );
    assert_eq!(report.changes[0].after, "balanceMinor");
    assert_eq!(
        compatibility(&a, &a).old_client_new_provider,
        Verdict::Compatible
    );
}

#[path = "support/http_provider.rs"]
mod live;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

const RELEASES: &str =
    "contracts/cache/http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4";
fn success(cmd: &mut Command) {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?}\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
fn project(v: &str) -> PathBuf {
    Path::new("examples/http/contract").join(v)
}
fn save(name: &str, e: &Evidence) {
    let dir = Path::new("target/http-conformance");
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), serde_json::to_vec_pretty(e).unwrap()).unwrap();
}

#[test]
fn real_version_matrix_and_v2_corpus() {
    let temp = tempfile::tempdir().unwrap();
    let mut servers = vec![];
    let fixture =
        json!([{"id":"A","owner":"Alice","balance":"10"},{"id":"B","owner":"Bob","balance":"7"}]);
    for v in ["v1", "v2"] {
        let root = project(v);
        let pin = root.join("workspace.lock.json");
        let cache = Path::new("contracts/cache");
        let build = live::ProviderBuild::generate(
            &root.join("project.ttl"),
            &pin,
            cache,
            Path::new("examples/http/provider/overlay.json"),
        );
        let bin = build.build();
        let client = temp.path().join(v);
        let c = load(&root.join("project.ttl"), &pin, cache).unwrap();
        emit_typescript(&c, &client).unwrap();
        success(
            Command::new("target/http-tools/node_modules/.bin/tsc")
                .args(["--strict", "--target", "ES2022", "--module", "commonjs"])
                .arg(client.join("client.ts")),
        );
        servers.push(live::Provider::launch(&bin, &fixture));
        if v == "v2" {
            let bundle = Path::new(RELEASES).join(v);
            let b = verify_bundle(&bundle, &pin).unwrap();
            let claim: Claim = read_json(Path::new("examples/http/provider/claim.json")).unwrap();
            let mut results = Vec::new();
            for case in b.cases.iter().filter(|c| c.role == "http.provider") {
                let server = live::Provider::launch(&bin, &case.fixture);
                let observations = case.action["requests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| server.exchange(r))
                    .collect::<Vec<_>>();
                assert!(
                    compare_observations(&case.expected, &observations),
                    "{}",
                    case.case_id
                );
                results.push(CaseResult {
                    case_id: case.case_id.clone(),
                    outcome: "pass".into(),
                    observations,
                });
            }
            assert_eq!(results.len(), 117);
            let e = Evidence {
                schema_version: 1,
                release: b.pin.clone(),
                rule_set_version: b.rule_set_version,
                claims: claim.claims.clone(),
                results,
            };
            verify_evidence(&b, &claim, &e).unwrap();
            save("provider-v2.json", &e);
            let evidence = client.join("evidence.json");
            let claimfile = Path::new("examples/http/clients/typescript/claim.json");
            success(
                Command::new("node")
                    .arg("examples/http/clients/typescript/peer.mjs")
                    .arg(client.join("client.js"))
                    .args([bundle.as_path(), &pin, claimfile, &evidence]),
            );
            let e: Evidence = read_json(&evidence).unwrap();
            assert_eq!(e.results.len(), 193);
            verify_evidence(&b, &read_json(claimfile).unwrap(), &e).unwrap();
            save("typescript-v2.json", &e);
        }
    }
    success(
        Command::new("node")
            .arg("tests/support/http-skew.cjs")
            .arg(temp.path())
            .args(servers.iter().map(|s| format!("http://{}", s.addr))),
    );
}

#[test]
fn external_go_replays_original_release_and_rejects_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let go = Path::new("examples/http/clients/go");
    for name in [
        "client.go",
        "go.mod",
        "peer_test.go",
        "claim.json",
        "target.lock.json",
    ] {
        fs::copy(go.join(name), dir.join(name)).unwrap();
    }
    let target = dir.join("target.lock.json");
    let bundle = dir.join("v1");
    publish(&source("v1"), &bundle).unwrap();
    let b = overlay::verify_target_lock(dir, &target, &bundle).unwrap();
    assert_eq!(
        fs::read(bundle.join("manifest.json")).unwrap(),
        fs::read(Path::new(RELEASES).join("v1/manifest.json")).unwrap()
    );
    assert_eq!(
        b.pin.manifest_sha256,
        "e1cf1857145ff54c538de9f14938fae607e1e925a5220dcc9feafb92aa60266a"
    );
    let evidence = dir.join("evidence.json");
    success(
        Command::new("go")
            .current_dir(dir)
            .args(["test", "-count=1", "."])
            .env("GOTOOLCHAIN", "local")
            .env("GOWORK", "off")
            .env("GOPROXY", "off")
            .env_remove("PIN")
            .env("BUNDLE", &bundle)
            .env("CLAIM", dir.join("claim.json"))
            .env("EVIDENCE", &evidence)
            .env_remove("SMOKE"),
    );
    let e: Evidence = read_json(&evidence).unwrap();
    verify_evidence(&b, &read_json(&dir.join("claim.json")).unwrap(), &e).unwrap();
    save("go-v1.json", &e);
    fs::write(dir.join("client.go"), "package ledgerclient\n").unwrap();
    assert!(
        overlay::verify_target_lock(dir, &target, &bundle)
            .unwrap_err()
            .contains("digest mismatch for client.go")
    );
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

#[test]
fn repinned_constraint_selects_dependent_reader_and_writer_cases() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    copy_dir(Path::new("contracts/cache"), &cache);
    let root = temp.path().join("source");
    copy_dir(&project("v2"), &root);
    let file = root.join("domain.ttl");
    let ttl = fs::read_to_string(&file)
        .unwrap()
        .replace("h:min 0", "h:min 1")
        .replace("h:version \"v2\"", "h:version \"bounds-probe\"");
    fs::write(&file, ttl).unwrap();
    let c = load_source(&root, &root.join("dependencies.lock.json"), &cache).unwrap();
    let out = cache
        .join(Path::new(RELEASES).strip_prefix("contracts/cache").unwrap())
        .join(&c.version);
    publish_successor(&c, &project("v2").join("workspace.lock.json"), &cache, &out).unwrap();
    let updated = load(
        &root.join("project.ttl"),
        &out.join("workspace.lock.json"),
        &cache,
    )
    .unwrap();
    let diff = compatibility(&source("v2"), &updated);
    assert_eq!(diff.old_client_new_provider, Verdict::Unknown);
    assert_eq!(diff.new_client_old_provider, Verdict::Unknown);
    assert_eq!(diff.changes.len(), 1);
    let changed = Iri::new(&diff.changes[0].declaration).unwrap();
    assert_eq!(changed.as_str(), "urn:oddlang:ledger-http:BalanceBounds");
    let rows = affected(&updated, &[changed]);
    let b = verify_bundle(&out, &out.join("workspace.lock.json")).unwrap();
    assert!(
        rows.iter()
            .any(|row| row.position == "behavior" && row.rule.ends_with(":transfer"))
    );
    for role in ["http.consumer", "http.provider"] {
        for position in [
            "response/balanceMinor",
            "response/source/balanceMinor",
            "response/destination/balanceMinor",
        ] {
            let row = rows
                .iter()
                .find(|o| {
                    o.role == role
                        && o.position == position
                        && o.declaration == "urn:oddlang:ledger-http:balance"
                })
                .unwrap();
            assert!(
                b.cases
                    .iter()
                    .any(|c| c.slots.contains(&row.witness("large")))
            );
        }
    }
    assert!(affected(&updated, &[Iri::new("urn:example:unrelated").unwrap()]).is_empty());
    let previous: Value = read_json(&project("v2").join("workspace.lock.json")).unwrap();
    let metadata: Value = read_json(&out.join("compatibility.json")).unwrap();
    assert_eq!(metadata["predecessor"], previous["pins"][0]);
    assert_eq!(metadata["compatibility"], json!(diff));
}

#[test]
fn workspace_resolves_multiple_http_identities_and_rejects_conflicting_pins() {
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    copy_dir(Path::new("contracts/cache"), &cache);
    let root = temp.path().join("source");
    copy_dir(&project("v1"), &root);
    let file = root.join("domain.ttl");
    let id = "urn:test:independent-http";
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("urn:oddlang:contract:ledger-http", id),
    )
    .unwrap();
    let other = load_source(&root, &root.join("dependencies.lock.json"), &cache).unwrap();
    let out = cache.join(format!(
        "http/{}/v1",
        Sha256::digest(id.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    publish(&other, &out).unwrap();
    let projectfile = temp.path().join("project.ttl");
    fs::write(&projectfile,format!("<urn:test:project> <urn:oddlang:core:imports> <urn:oddlang:contract:ledger-http>, <{id}> .")).unwrap();
    let mut lock: Value = read_json(&project("v2").join("workspace.lock.json")).unwrap();
    let otherlock: Value = read_json(&out.join("workspace.lock.json")).unwrap();
    lock["pins"]
        .as_array_mut()
        .unwrap()
        .push(otherlock["pins"][0].clone());
    let lockfile = temp.path().join("workspace.lock.json");
    fs::write(&lockfile, serde_json::to_vec(&lock).unwrap()).unwrap();
    assert_eq!(
        load_workspace(&projectfile, &lockfile, &cache)
            .unwrap()
            .len(),
        2
    );
    assert!(
        load(&projectfile, &lockfile, &cache)
            .unwrap_err()
            .contains("one HTTP")
    );
    let mut conflict = otherlock["pins"][0].clone();
    conflict["version"] = json!("different");
    lock["pins"].as_array_mut().unwrap().push(conflict);
    fs::write(&lockfile, serde_json::to_vec(&lock).unwrap()).unwrap();
    assert!(
        load_workspace(&projectfile, &lockfile, &cache)
            .unwrap_err()
            .contains("conflicting pins")
    );
}
