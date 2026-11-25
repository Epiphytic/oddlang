use std::path::Path;
#[test]
fn fetch_binding() {
    oddlang::http::load_overlay(Path::new("examples/http/clients/typescript/overlay.json"))
        .unwrap();
}

#[allow(dead_code)]
#[path = "support/http_provider.rs"]
mod provider;
use oddlang::http::{Claim, Evidence, read_json, verify_bundle, verify_evidence};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}
fn success(cmd: &mut Command) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{cmd:?}\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
struct Runner {
    language: &'static str,
    root: PathBuf,
    bundle: PathBuf,
    pin: PathBuf,
    claim: PathBuf,
    evidence: PathBuf,
}
impl Runner {
    fn source(&self) -> PathBuf {
        self.root.join(if self.language == "go" {
            "client.go"
        } else {
            "client.ts"
        })
    }
    fn compile(&self) {
        if self.language == "typescript" {
            success(
                Command::new("target/http-tools/node_modules/.bin/tsc")
                    .args(["--strict", "--target", "ES2022", "--module", "commonjs"])
                    .arg(self.source()),
            );
        }
    }
    fn command(&self, smoke: Option<&str>) -> Command {
        if self.language == "go" {
            let mut cmd = Command::new("go");
            cmd.current_dir(&self.root)
                .args(["test", "-count=1", "."])
                .env("GOWORK", "off")
                .env("GOPROXY", "off")
                .env("BUNDLE", &self.bundle)
                .env("PIN", &self.pin)
                .env("CLAIM", &self.claim)
                .env("EVIDENCE", &self.evidence)
                .env("SMOKE", smoke.unwrap_or(""));
            cmd
        } else {
            let mut cmd = Command::new("node");
            cmd.current_dir(&self.root)
                .arg("peer.mjs")
                .arg("client.js")
                .args([&self.bundle, &self.pin, &self.claim, &self.evidence]);
            if let Some(url) = smoke {
                cmd.arg(url);
            }
            cmd
        }
    }
    fn run(&self) -> Evidence {
        success(&mut self.command(None));
        read_json(&self.evidence).unwrap()
    }
}

#[test]
fn standalone_clients_against_scripted_peers_and_provider() {
    let temp = tempfile::tempdir().unwrap();
    let project = Path::new("examples/http/contract/v1");
    let cache = Path::new("contracts/cache");
    let bundle = temp.path().join("release");
    copy_tree(
        &cache.join("http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4/v1"),
        &bundle,
    );
    let pin = temp.path().join("workspace.lock.json");
    fs::copy(project.join("workspace.lock.json"), &pin).unwrap();
    let verified = verify_bundle(&bundle, &pin).unwrap();
    let build = provider::ProviderBuild::generate(
        &project.join("project.ttl"),
        &pin,
        cache,
        Path::new("examples/http/provider/overlay.json"),
    );
    let server = provider::Provider::launch(
        &build.build(),
        &json!([{"id":"A","owner":"Alice","balance":"9007199254740993"}]),
    );
    for language in ["typescript", "go"] {
        let root = temp.path().join(language);
        let fixture = Path::new("examples/http/clients").join(language);
        copy_tree(&fixture, &root);
        let runner = Runner {
            language,
            claim: root.join("claim.json"),
            evidence: root.join("evidence.json"),
            bundle: bundle.clone(),
            pin: pin.clone(),
            root,
        };
        if language == "typescript" {
            success(
                Command::new(env!("CARGO_BIN_EXE_oddlang"))
                    .args(["http", "generate"])
                    .args([
                        project.join("project.ttl"),
                        pin.clone(),
                        cache.into(),
                        fixture.join("overlay.json"),
                        runner.root.clone(),
                    ]),
            );
            assert_eq!(
                fs::read(runner.source()).unwrap(),
                fs::read(fixture.join("generated/client.ts")).unwrap()
            );
        }
        runner.compile();
        let stock = runner.run();
        let claim: Claim = read_json(&runner.claim).unwrap();
        assert_eq!(stock.results.len(), 193);
        verify_evidence(&verified, &claim, &stock).unwrap();
        success(&mut runner.command(Some(&format!("http://{}", server.addr))));
        let smoke: Value = read_json(&runner.evidence).unwrap();
        assert_eq!(
            smoke,
            json!({"kind":"success","value":{"id":"A","owner":"Alice","balance":"9007199254740993"}})
        );

        let source = fs::read_to_string(runner.source()).unwrap();
        let mutations = if language == "go" {
            [
                (
                    "strconv.FormatInt(amount, 10)",
                    "strconv.FormatFloat(float64(amount), 'f', 0, 64)",
                ),
                ("|| n < 0 {", "|| n < 0 || n == 9223372036854775807 {"),
            ]
        } else {
            [
                (
                    "encode ? n.toString() : n",
                    "encode ? BigInt(Number(n)).toString() : n",
                ),
                (
                    "n > BigInt(f.max)",
                    "n > BigInt(f.max) || (!encode && n === 9223372036854775807n)",
                ),
            ]
        };
        for (i, (needle, replace)) in mutations.into_iter().enumerate() {
            assert_eq!(source.matches(needle).count(), 1);
            fs::write(runner.source(), source.replace(needle, replace)).unwrap();
            runner.compile();
            fs::remove_file(&runner.evidence).unwrap();
            let output = runner.command(None).output().unwrap();
            assert!(!output.status.success());
            let mutant: Evidence = read_json(&runner.evidence).unwrap();
            let mut affected = 0;
            for (j, r) in mutant.results.iter().enumerate() {
                let case = verified
                    .cases
                    .iter()
                    .find(|c| c.case_id == r.case_id)
                    .unwrap();
                let targeted = if i == 0 {
                    case.action["input"]["amount"].as_str().is_some_and(|s| {
                        let amount = s.parse::<i64>().unwrap();
                        (amount as f64) as i128 != i128::from(amount)
                    })
                } else {
                    case.expected.last().unwrap()["native"]["value"]
                        .to_string()
                        .contains("\"9223372036854775807\"")
                };
                assert_eq!(r.case_id, stock.results[j].case_id);
                if targeted {
                    affected += 1;
                    let mut one = stock.clone();
                    one.results[j] = r.clone();
                    one.results[j].outcome = "pass".into();
                    assert_eq!(
                        verify_evidence(&verified, &claim, &one).unwrap_err(),
                        format!("failed observations {}", r.case_id)
                    );
                    assert_eq!(r.outcome, "fail");
                } else {
                    assert_eq!(r.outcome, "pass");
                    assert_eq!(r.observations, stock.results[j].observations);
                }
            }
            assert_eq!(mutant.results.len(), 193);
            assert!(affected > 0 && affected < 193);
            println!(
                "{language} {} affected {affected} of 193",
                if i == 0 { "encoder" } else { "decoder" }
            );
        }
        let path = bundle.join("cases.json");
        let original = fs::read(&path).unwrap();
        let mut tampered = original.clone();
        tampered.push(b' ');
        fs::write(&path, tampered).unwrap();
        let Output {
            status,
            stdout,
            stderr,
        } = runner.command(None).output().unwrap();
        assert!(!status.success());
        assert!(
            format!(
                "{}{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            )
            .contains("payload digest mismatch")
        );
        fs::write(path, original).unwrap();
    }
}
