#[path = "support/ledger_oracle.rs"]
mod ledger_oracle;

use ledger_oracle::{Account, State};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::tempdir;

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn digest(path: &Path) -> String {
    Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
fn rows(values: &Value) -> State {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let id = text(&row["id"]).to_owned();
            (
                id.clone(),
                Account {
                    id,
                    owner: text(&row["owner"]).to_owned(),
                    balance: text(&row["balance"]).parse().unwrap(),
                },
            )
        })
        .collect()
}
fn row(account: &Account) -> Value {
    json!({"id":account.id, "owner":account.owner, "balance":account.balance.to_string()})
}

/// Recompute each expected observation from the native spec oracle, independently of
/// both component implementations and the portable corpus consumed by all three hosts.
#[test]
fn portable_cases_match_independent_ledger_oracle() {
    for world in ["ledger", "ledger-test"] {
        let corpus = read(&Path::new("contracts/wasm").join(world).join("corpus.json"));
        let mut errors = BTreeSet::new();
        for case in corpus.as_array().unwrap() {
            let mut state = rows(&case["imports"][0]["returns"]);
            let mut pending = BTreeSet::new();
            for call in case["calls"].as_array().unwrap() {
                let args = &call["arguments"];
                let expected = match text(&call["function"]) {
                    "transfer" => {
                        assert!(pending.is_empty(), "unobserved accounts {pending:?}");
                        pending.extend(state.keys().cloned());
                        pending.insert("missing".to_owned());
                        match ledger_oracle::transfer(
                            &mut state,
                            text(&args[0]),
                            text(&args[1]),
                            text(&args[2]).parse().unwrap(),
                        ) {
                            Ok(()) => json!({"ok":null}),
                            Err(iri) => {
                                let name = iri.strip_prefix("urn:oddlang:ledger:error:").unwrap();
                                let mut tag = String::new();
                                for c in name.chars() {
                                    if c.is_uppercase() && !tag.is_empty() {
                                        tag.push('-');
                                    }
                                    tag.push(c.to_ascii_lowercase());
                                }
                                errors.insert(tag.clone());
                                json!({"err":tag})
                            }
                        }
                    }
                    "lookup" => {
                        let id = text(&args[0]);
                        pending.remove(id);
                        state
                            .get(id)
                            .map_or_else(|| json!({"none":true}), |a| json!({"some":row(a)}))
                    }
                    "fixture-load" if world == "ledger-test" => {
                        state = rows(&args[0]);
                        Value::Null
                    }
                    "complete-snapshot" if world == "ledger-test" => {
                        json!(state.values().map(row).collect::<Vec<_>>())
                    }
                    other => panic!("unexpected operation {other}"),
                };
                assert_eq!(call["expected"], expected, "{} {call}", case["caseId"]);
            }
            assert!(pending.is_empty(), "unobserved accounts {pending:?}");
        }
        assert_eq!(errors.len(), 6);
    }
}

struct Runtime {
    root: PathBuf,
    reports: PathBuf,
}

impl Runtime {
    fn bundle(&self, world: &str) -> PathBuf {
        self.root.join(world)
    }
    fn lock(&self, world: &str) -> PathBuf {
        let lock = self.reports.join(format!("{world}.lock.json"));
        if !lock.exists() {
            fs::copy(self.bundle(world).join("workspace.lock.json"), &lock).unwrap();
        }
        lock
    }
    fn component(&self, guest: &str) -> PathBuf {
        self.root.join("guests").join(format!("{guest}.wasm"))
    }
    fn claim(&self, world: &str, guest: &str, roles: &[&str]) -> PathBuf {
        let bundle = self.bundle(world);
        let name = if world.starts_with("echo") {
            "echo-world"
        } else if world == "ledger-probe" {
            "ledger"
        } else {
            world
        };
        let path = self.reports.join(format!("{world}-{guest}.claim.json"));
        write(
            &path,
            &json!({
                "participant":guest, "roles":roles, "world":name,
                "releaseSha256":digest(&bundle.join("manifest.json")),
                "runnerSha256":digest(&self.root.join("runtime-host")),
                "componentSha256":digest(&self.component(guest)),
            }),
        );
        path
    }
    fn command(&self, operation: &str, args: &[&Path]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(["wasm", operation])
            .args(args)
            .env("ODDLANG_WASM_HOST", self.root.join("runtime-host"))
            .env("WASM_TOOLS", self.root.join("wasm-tools"))
            .output()
            .unwrap()
    }
    fn run(&self, world: &str, guest: &str, claim: &Path, report: &Path) -> Output {
        let bundle = self.bundle(world);
        self.command(
            "run",
            &[
                &bundle,
                &self.lock(world),
                claim,
                &self.component(guest),
                report,
            ],
        )
    }
    fn verify(&self, world: &str, claim: &Path, report: &Path) -> Output {
        let bundle = self.bundle(world);
        self.command(
            "verify-evidence",
            &[&bundle, &self.lock(world), claim, report],
        )
    }
}

fn success(result: Output) {
    assert!(
        result.status.success(),
        "stdout {}\nstderr {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
fn failure(result: Output) {
    assert!(
        !result.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&result.stdout)
    );
}

#[test]
fn exercise_released_components_and_reject_counterexamples() {
    let temp = tempdir().unwrap();
    let root = match env::var_os("ODDLANG_WASM_ARTIFACTS") {
        Some(path) => PathBuf::from(path),
        None => {
            let root = temp.path().join("built");
            success(
                Command::new("scripts/verify-wasm.sh")
                    .arg("--prepare")
                    .arg(&root)
                    .output()
                    .unwrap(),
            );
            root
        }
    };
    let reports = root.join("wasmtime-reports");
    fs::create_dir_all(&reports).unwrap();
    let rt = Runtime { root, reports };
    let roles = ["wasm.exporter", "wasm.importer"];
    for world in ["ledger", "ledger-test"] {
        for implementation in ["manual", "generated"] {
            let guest = format!("{implementation}-{world}");
            let claim = rt.claim(world, &guest, &roles);
            let report = rt.reports.join(format!("{guest}.json"));
            success(rt.run(world, &guest, &claim, &report));
            success(rt.verify(world, &claim, &report));
            match_published_cases(&rt.bundle(world), &report);
        }
    }

    let claim = rt.claim("ledger", "manual-ledger", &roles);
    failure(rt.verify(
        "ledger",
        &claim,
        &rt.reports.join("manual-ledger-test.json"),
    ));
    let original = read(&rt.reports.join("manual-ledger.json"));
    let edited = temp.path().join("edited.json");
    for field in [
        "releaseSha256",
        "componentSha256",
        "runnerSha256",
        "participant",
        "world",
    ] {
        let mut stale = original.clone();
        stale["claim"][field] = json!("stale");
        write(&edited, &stale);
        failure(rt.verify("ledger", &claim, &edited));
    }
    let mut partial = original.clone();
    partial["cases"].as_array_mut().unwrap().pop().unwrap();
    write(&edited, &partial);
    failure(rt.verify("ledger", &claim, &edited));

    let mut altered = original.clone();
    altered["cases"][0]["imports"][0]["returns"] = json!([]);
    write(&edited, &altered);
    failure(rt.verify("ledger", &claim, &edited));
    let mut duplicate = original.clone();
    let duplicate_case = duplicate["cases"][0].clone();
    duplicate["cases"]
        .as_array_mut()
        .unwrap()
        .push(duplicate_case);
    write(&edited, &duplicate);
    failure(rt.verify("ledger", &claim, &edited));

    let wrong = rt.claim("ledger", "manual-ledger-test", &roles);
    failure(rt.run(
        "ledger",
        "manual-ledger-test",
        &wrong,
        &temp.path().join("wrong.json"),
    ));

    for guest in ["trap", "double-import"] {
        let claim = rt.claim("ledger", guest, &roles);
        let report = rt.reports.join(format!("{guest}.json"));
        // Failed runs retain the actual observations for diagnosis.
        if report.exists() {
            fs::remove_file(&report).unwrap();
        }
        let output = rt.run("ledger", guest, &claim, &report);
        assert!(
            report.is_file(),
            "missing {guest} evidence: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let observed = read(&report);
        let cases = observed["cases"].as_array().unwrap();
        if guest == "trap" {
            assert!(
                cases
                    .iter()
                    .flat_map(|case| case["calls"].as_array().unwrap())
                    .any(|call| call["function"] == "transfer"
                        && call.get("trap").is_some()
                        && call.get("result").is_none())
            );
        } else {
            assert!(cases.iter().any(|case| {
                case["imports"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|call| call["function"] == "initial-state")
                    .count()
                    == 2
            }));
        }
        failure(rt.verify("ledger", &claim, &report));
    }
    let probe_claim = rt.claim("ledger-probe", "probe", &roles);
    let probe_report = rt.reports.join("probe.json");
    success(rt.run("ledger-probe", "probe", &probe_claim, &probe_report));
    success(rt.verify("ledger-probe", &probe_claim, &probe_report));
    reject_invalid_imports(&rt, temp.path(), &claim);
    matrix(&rt);
}

/// Check the actual observations independently of the CLI evidence verifier.
fn match_published_cases(bundle: &Path, report: &Path) {
    let published = read(&bundle.join("cases.json"));
    let observed = read(report);
    let traces = observed["cases"].as_array().unwrap();
    assert_eq!(traces.len(), published.as_array().unwrap().len());
    for case in published.as_array().unwrap() {
        let trace = traces
            .iter()
            .find(|trace| trace["caseId"] == case["caseId"])
            .unwrap();
        assert_eq!(trace["imports"], case["imports"], "{}", case["caseId"]);
        let actual = trace["calls"].as_array().unwrap();
        let expected = case["calls"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len(), "{}", case["caseId"]);
        for (got, want) in actual.iter().zip(expected) {
            assert_eq!(
                got,
                &json!({"function":want["function"], "arguments":want["arguments"], "result":want["expected"]}),
                "{}",
                case["caseId"]
            );
        }
    }
}

fn reject_invalid_imports(rt: &Runtime, temp: &Path, original_claim: &Path) {
    let source = temp.join("incomplete-script");
    fs::create_dir(&source).unwrap();
    let fixture = Path::new("contracts/wasm/ledger");
    for name in ["model.ttl", "dependencies.json"] {
        fs::copy(fixture.join(name), source.join(name)).unwrap();
    }
    let mut cases = read(&fixture.join("corpus.json"));
    cases[1]["imports"] = json!([]);
    write(&source.join("corpus.json"), &cases);
    let bundle = temp.join("unprovided-import");
    let publish = |out: &Path| {
        rt.command(
            "publish",
            &[
                &source,
                Path::new("contracts/wasm/empty.lock.json"),
                Path::new("contracts/cache"),
                out,
            ],
        )
    };
    success(publish(&bundle));

    let mut pinned = read(original_claim);
    pinned["releaseSha256"] = json!(digest(&bundle.join("manifest.json")));
    let claim = temp.join("unprovided-import.claim.json");
    write(&claim, &pinned);
    let pin = temp.join("unprovided-import.lock.json");
    fs::copy(bundle.join("workspace.lock.json"), &pin).unwrap();
    let result = rt.command(
        "run",
        &[
            &bundle,
            &pin,
            &claim,
            &rt.component("manual-ledger"),
            &temp.join("unprovided.json"),
        ],
    );
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("import"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    failure(result);
    cases[1]["imports"] = json!([{"function":"private-clock","arguments":[],"returns":"1"}]);
    write(&source.join("corpus.json"), &cases);
    let result = publish(&temp.join("undeclared-import"));
    assert!(String::from_utf8_lossy(&result.stderr).contains("private-clock"));
    failure(result);
}

fn matrix(rt: &Runtime) {
    for (edit, old_host, new_host) in [
        ("label", "observed-compatible", "observed-compatible"),
        ("rename", "type-failure", "type-failure"),
        ("tags", "type-failure", "type-failure"),
        ("import", "link-failure", "behavior-failure"),
    ] {
        let new = format!("echo-{edit}");
        for (direction, host, guest_bundle, expected) in [
            ("old-host-new-guest", "echo", new.as_str(), old_host),
            ("new-host-old-guest", new.as_str(), "echo", new_host),
        ] {
            let guest = if guest_bundle == "echo-label" {
                "echo"
            } else {
                guest_bundle
            };
            let roles: &[&str] = if guest_bundle == "echo-import" {
                &["wasm.exporter", "wasm.importer"]
            } else {
                &["wasm.exporter"]
            };
            let claim = rt.claim(guest_bundle, guest, roles);
            let host_lock = rt.lock(host);
            let host = rt.bundle(host);
            let release = rt.bundle(guest_bundle);
            let report = rt.reports.join(format!("{edit}-{direction}.json"));
            if report.exists() {
                fs::remove_file(&report).unwrap();
            }
            let output = rt.command(
                "matrix",
                &[
                    &host,
                    &host_lock,
                    &release,
                    &rt.lock(guest_bundle),
                    &claim,
                    &rt.component(guest),
                    Path::new(direction),
                    &report,
                ],
            );
            if !report.is_file() {
                success(output);
            }
            let evidence = read(&report);
            assert_eq!(evidence["direction"], direction);
            assert_eq!(
                evidence["guestReleaseSha256"],
                digest(&release.join("manifest.json"))
            );
            assert_eq!(
                evidence["hostReleaseSha256"],
                digest(&host.join("manifest.json"))
            );
            assert_eq!(evidence["claim"], read(&claim));
            assert_eq!(evidence["caseIds"], json!(["unicode-and-empty-values"]));
            if expected == "observed-compatible" {
                match_published_cases(&host, &report);
            }
            assert_eq!(
                evidence["verdict"], expected,
                "{edit}/{direction}: {evidence}"
            );
        }
    }
}
