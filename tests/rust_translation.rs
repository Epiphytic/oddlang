#[path = "support/rust_translation/corpus.rs"]
mod corpus;
#[path = "support/ledger_oracle.rs"]
mod ledger_oracle;

use corpus::Case;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const RUNNER: &str = "tests/support/rust_translation/runner.rs";

fn hash(file: &Path) -> String {
    Sha256::digest(fs::read(file).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn success(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn copy_crate(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for e in fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        if e.file_name() == "target" {
            continue;
        }
        if e.file_type().unwrap().is_dir() {
            copy_crate(&e.path(), &dest.join(e.file_name()));
        } else {
            fs::copy(e.path(), dest.join(e.file_name())).unwrap();
        }
    }
}

fn regenerate(src: &Path, dir: &Path, accepted: Option<(&Path, &Path)>) -> PathBuf {
    let project = dir.join("project");
    let native = dir.join("native");
    fs::create_dir_all(dir).unwrap();
    let cli = || Command::new(env!("CARGO_BIN_EXE_oddlang"));
    let mut import = cli();
    import
        .args(["import", "rust"])
        .arg(src.join("Cargo.toml"))
        .arg(src.join("mapping.toml"))
        .arg(&project);
    if let Some((receipt, pin)) = accepted {
        import
            .arg("--accepted-cleanup")
            .arg(receipt)
            .arg("--baseline-pin")
            .arg(pin);
    }
    success(&mut import);
    success(cli().arg("build").arg(&project).arg(&native));
    success(
        cli()
            .args(["import", "verify-build"])
            .arg(&project)
            .arg(&native),
    );
    native.join("account.rs")
}

fn decode(text: &str) -> String {
    assert!(text.len().is_multiple_of(2));
    String::from_utf8(
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect(),
    )
    .unwrap()
}

fn observe(dir: &Path, source: &Path, original: bool, count: usize) -> Vec<Value> {
    let binary = dir.join("observe");
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition=2024", "-Coverflow-checks=yes", "-Awarnings"]);
    if original {
        rustc.args(["--cfg", "original"]);
    }
    success(
        rustc
            .env("TRANSLATION_SOURCE", source)
            .arg(Path::new(ROOT).join(RUNNER))
            .arg("-o")
            .arg(&binary),
    );
    let output = success(
        Command::new(binary).stdin(Stdio::from(fs::File::open(dir.join("corpus")).unwrap())),
    );
    let obs: Vec<_> = output
        .lines()
        .map(|line| {
            let mut parts = line.split('\t');
            let status = parts.next().unwrap();
            let mut rows = serde_json::Map::new();
            while let Some(key) = parts.next() {
                let id = decode(parts.next().unwrap());
                let owner = decode(parts.next().unwrap());
                let balance: i64 = parts.next().unwrap().parse().unwrap();
                assert!(
                    rows.insert(
                        decode(key),
                        json!({ "id":id,"owner":owner,"balance":balance })
                    )
                    .is_none()
                );
            }
            json!({"status":status,"rows":rows})
        })
        .collect();
    assert_eq!(obs.len(), count);
    obs
}

fn compare(cases: &[Case], expected: &[Value], actual: &[Value], label: &str) {
    for (i, (a, b)) in expected.iter().zip(actual).enumerate() {
        assert_eq!(a, b, "{label}, case {i}: {}", cases[i].input());
    }
}

fn counterexample(case: &Case, expected: &Value, actual: &Value) -> Value {
    assert_ne!(expected, actual);
    json!({"input":case.input(), "expected":expected, "actual":actual})
}

#[test]
fn original_and_regenerated_transfers_match_the_oracle() {
    let root = Path::new(ROOT);
    let fixture = root.join("examples/rust-import/ledger");
    let original = fixture.join("src/lib.rs");
    let pins: BTreeMap<_, _> = include_str!("../examples/rust-import/ledger/original.sha256")
        .lines()
        .map(|s| {
            let (digest, name) = s.split_once("  ").unwrap();
            (name, digest)
        })
        .collect();
    for name in ["src/lib.rs", "Cargo.toml", "Cargo.lock"] {
        assert_eq!(hash(&fixture.join(name)), pins[name]);
    }
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path();
    let cases = corpus::cases();
    let count = cases.len();
    let inputs = cases.iter().map(Case::wire).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(dir.join("corpus"), inputs).unwrap();
    let expected: Vec<_> = cases.iter().map(Case::observation).collect();
    let mut outcomes = BTreeMap::new();
    for obs in &expected {
        *outcomes
            .entry(obs["status"].as_str().unwrap())
            .or_insert(0usize) += 1;
    }
    for name in [
        "SourceMissing",
        "DestinationMissing",
        "SameAccount",
        "NonPositiveAmount",
        "InsufficientFunds",
        "DestinationOverflow",
    ] {
        assert!(outcomes.contains_key(format!("urn:oddlang:ledger:error:{name}").as_str()));
    }
    assert!(outcomes.contains_key("ok"));
    assert_eq!(outcomes.len(), 7);

    let generated = regenerate(&fixture, &dir.join("baseline"), None);
    let baseline = observe(dir, &original, true, count);
    compare(&cases, &expected, &baseline, "frozen original");
    compare(
        &cases,
        &expected,
        &observe(dir, &generated, false, count),
        "regenerated",
    );

    // Re-import a digest-accepted source edit and execute the archived pre-edit source.
    let cleanup = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_oddlang"));
        c.arg("cleanup")
            .current_dir(dir);
        c
    };
    success(cleanup().arg("freeze").arg(&fixture).arg("baseline.pin"));
    let edit = dir.join("edit");
    copy_crate(&fixture, &edit);
    let source = fs::read_to_string(&original).unwrap();
    fs::write(
        edit.join("src/lib.rs"),
        source.replace("credit_before", "previous_credit"),
    )
    .unwrap();
    let digest = success(
        cleanup()
            .arg("propose")
            .arg(&fixture)
            .arg(&edit)
            .arg("receipt"),
    );
    success(cleanup().args(["accept", "receipt", "--sha256", digest.trim()]));
    let receipt = dir.join("receipt");
    let archived = receipt.join("original/src/lib.rs");
    let edited = receipt.join("effective");
    let cleaned = regenerate(
        &edited,
        &dir.join("cleaned"),
        Some((&receipt.join("accepted.json"), &dir.join("baseline.pin"))),
    );
    let before = observe(dir, &archived, true, count);
    compare(&cases, &expected, &before, "archived original");
    compare(
        &cases,
        &before,
        &observe(dir, &cleaned, false, count),
        "accepted cleanup against pre-edit source",
    );
    assert_eq!(hash(&archived), hash(&original));
    assert_ne!(hash(&archived), hash(&edited.join("src/lib.rs")));

    // Import the changed guard priority. The counterexample must survive generation.
    let amount_guard =
        "    if amount <= 0 {\n        return Err(TransferError::NonPositiveAmount);\n    }\n";
    assert_eq!(source.matches(amount_guard).count(), 1);
    let changed = source.replace(amount_guard, "").replace(
        "    if !state.contains_key(source)",
        &format!("{amount_guard}    if !state.contains_key(source)"),
    );
    fs::write(edit.join("src/lib.rs"), changed).unwrap();
    let reordered = regenerate(&edit, &dir.join("priority"), None);
    let priority = observe(dir, &reordered, false, count);
    assert_eq!(
        priority[0]["status"],
        "urn:oddlang:ledger:error:NonPositiveAmount"
    );
    assert_eq!(
        expected[0]["status"],
        "urn:oddlang:ledger:error:SourceMissing"
    );
    let guard_witness = counterexample(&cases[0], &expected[0], &priority[0]);

    // Mutate emitted code at its overflow return. The returned error still agrees.
    let code = fs::read_to_string(&generated).unwrap();
    let anchor = "return Err(\"urn:oddlang:ledger:error:DestinationOverflow\");";
    assert_eq!(code.matches(anchor).count(), 1);
    let early = dir.join("early.rs");
    fs::write(
        &early,
        code.replace(
            anchor,
            &format!("state.0.get_mut(source).unwrap().balance.0 -= amount; {anchor}"),
        ),
    )
    .unwrap();
    let partial = observe(dir, &early, false, count);
    assert_eq!(
        expected[1]["status"],
        "urn:oddlang:ledger:error:DestinationOverflow"
    );
    assert_eq!(partial[1]["status"], expected[1]["status"]);
    assert_eq!(expected[1]["rows"][""]["balance"], 3);
    assert_eq!(partial[1]["rows"][""]["balance"], 2);
    let debit_witness = counterexample(&cases[1], &expected[1], &partial[1]);

    let lock = dir.join("baseline/project/import.lock");
    let manifest: Value = serde_json::from_slice(&fs::read(&lock).unwrap()).unwrap();
    assert_eq!(
        success(Command::new("rustc").arg("-vV")).trim(),
        manifest["compiler"].as_str().unwrap().trim()
    );
    if let Some(out) = env::var_os("ODDLANG_TRANSLATION_ARTIFACTS") {
        let destination = PathBuf::from(out);
        fs::create_dir_all(&destination).unwrap();
        for (name, path) in [
            ("cases", dir.join("corpus")),
            ("cleanup.patch", receipt.join("cleanup.patch")),
            ("effective.rs", edited.join("src/lib.rs")),
            ("cleaned.rs", cleaned.clone()),
            ("reordered-source.rs", edit.join("src/lib.rs")),
            ("reordered-generated.rs", reordered.clone()),
            ("early-debit.rs", early.clone()),
        ] {
            fs::copy(path, destination.join(name)).unwrap();
        }
    }
    let report = json!({
        "schemaVersion":1,"classification":"tested","role":"native-rust",
        "operation":"urn:oddlang:ledger:TransferFunds","cases":count,"outcomes":outcomes,
        "seed":format!("0x{:016x}",corpus::SEED),"randomCases":corpus::RANDOM_CASES,
        "artifacts":{"original":hash(&original),"generated":hash(&generated),"oracle":hash(&root.join("tests/support/ledger_oracle.rs")),
                     "harness":hash(&root.join("tests/rust_translation.rs")),"adapter":hash(&root.join(RUNNER)),
                     "corpusSource":hash(&root.join("tests/support/rust_translation/corpus.rs")),"cases":hash(&dir.join("corpus")),
                     "graph":manifest["graphSha256"],"importLock":hash(&lock),"reorderedSource":hash(&edit.join("src/lib.rs")),
                     "reorderedGenerated":hash(&reordered),"earlyDebitGenerated":hash(&early)},
        "mutants":{"guardOrder":guard_witness,"earlyDebit":debit_witness},
        "cleanup":{"originalSha256":hash(&archived),"effectiveSha256":hash(&edited.join("src/lib.rs")),
                   "patchSha256":hash(&receipt.join("cleanup.patch")),"generatedSha256":hash(&cleaned),"cases":count}
    });
    if let Some(path) = env::var_os("ODDLANG_TRANSLATION_REPORT") {
        fs::write(path, serde_json::to_string_pretty(&report).unwrap() + "\n").unwrap();
    }
}
