#[allow(dead_code)]
mod support_fixture;

use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use support_fixture::replace;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/ledger")
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn copy_fixture(dir: &Path) {
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "mapping.toml",
        "original.sha256",
        "overlays/fields.ttl",
        "overlays/rust.ttl",
        "overlays/sql.ttl",
        "overlays/avro.ttl",
    ] {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(fixture().join(relative), path).unwrap();
    }
}

fn cli(args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn failure(output: Output) -> String {
    assert!(!output.status.success());
    let message = String::from_utf8(output.stderr).unwrap();
    assert!(!message.trim().is_empty());
    message
}

fn import(src: &Path, project: &Path) -> Output {
    cli(&[
        "import".as_ref(),
        "rust".as_ref(),
        &src.join("Cargo.toml"),
        &src.join("mapping.toml"),
        project,
    ])
}

fn verify(project: &Path) -> Output {
    cli(&["import".as_ref(), "verify".as_ref(), project])
}

fn check_runner(src: &Path, expected_error: &str, expected_credit: u8) {
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    success(import(src, &project));
    success(verify(&project));
    let native = scratch.path().join("native");
    success(cli(&["build".as_ref(), &project, &native]));

    // The consumer sees only emitted Rust and the public operation registry.
    let consumer = scratch.path().join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::copy(native.join("account.rs"), consumer.join("src/account.rs")).unwrap();
    fs::write(
        consumer.join("src/main.rs"),
        include_str!("support/compiled_transfer.rs"),
    )
    .unwrap();
    fs::write(
        consumer.join("Cargo.toml"),
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n",
    )
    .unwrap();
    success(
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(&consumer)
            .output()
            .unwrap(),
    );
    let input = consumer.join("inputs");
    fs::write(&input, "10 2 7 3 4 -1\n10 2 7 0 1 3\n").unwrap();
    let output = success(
        Command::new("cargo")
            .args(["run", "--quiet", "--locked", "--offline"])
            .env("CARGO_TARGET_DIR", consumer.join("target"))
            .current_dir(consumer)
            .stdin(Stdio::from(fs::File::open(input).unwrap()))
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!(
            "urn:oddlang:ledger:error:{expected_error}\tA\tA\tAda\t10\tC\tC\tCy\t7\ta\ta\t\t2\nok\tA\tA\tAda\t7\tC\tC\tCy\t7\ta\ta\t\t{expected_credit}\n"
        )
    );
}

#[test]
fn independently_pinned_ledger_regenerates_an_executable() {
    let src = fixture();
    let mut remaining = vec!["Cargo.toml", "Cargo.lock", "src/lib.rs"];
    for line in fs::read_to_string(src.join("original.sha256"))
        .unwrap()
        .lines()
    {
        let (digest, name) = line.split_once("  ").unwrap();
        remaining.remove(remaining.iter().position(|p| *p == name).unwrap());
        assert_eq!(
            Sha256::digest(fs::read(src.join(name)).unwrap())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            digest
        );
    }
    assert!(remaining.is_empty());
    check_runner(&src, "SourceMissing", 5);
}

#[test]
fn guard_order_and_arithmetic_operands_reach_executable() {
    let scratch = tempfile::tempdir().unwrap();
    let src = scratch.path();
    copy_fixture(src);
    let file = "src/lib.rs";
    let source_guard = "    if !state.contains_key(source) {\n        return Err(TransferError::SourceMissing);\n    }\n";
    let destination_guard = "    if !state.contains_key(destination) {\n        return Err(TransferError::DestinationMissing);\n    }\n";
    replace(
        src,
        file,
        &format!("{source_guard}{destination_guard}"),
        &format!("{destination_guard}{source_guard}"),
    );
    check_runner(src, "DestinationMissing", 5);
    replace(
        src,
        file,
        "credit_before + amount_wide",
        "credit_before + debit_before",
    );
    check_runner(src, "DestinationMissing", 12);
}

#[test]
fn clean_directories_reproduce_publication_and_local_edits_preserve_turtle() {
    let scratch = tempfile::tempdir().unwrap();
    let a = scratch.path().join("first");
    let b = scratch.path().join("second");
    let project = scratch.path().join("out");
    let other = scratch.path().join("elsewhere");
    copy_fixture(&a);
    copy_fixture(&b);
    success(import(&a, &project));
    success(import(&b, &other));
    let baseline = files(&project);
    assert_eq!(baseline, files(&other));
    replace(&b, "src/lib.rs", "credit_before", "old_credit");
    replace(
        &b,
        "src/lib.rs",
        "pub fn transfer(",
        "// External author annotation.\n\npub fn transfer(",
    );
    let renamed = scratch.path().join("renamed");
    success(import(&b, &renamed));
    let edited = files(&renamed);
    assert_ne!(
        baseline[Path::new("import.lock")],
        edited[Path::new("import.lock")]
    );
    let turtle = |bundle: &BTreeMap<PathBuf, Vec<u8>>| {
        bundle
            .iter()
            .filter(|(name, _)| name.extension().is_some_and(|ext| ext == "ttl"))
            .map(|(name, bytes)| (name.clone(), bytes.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert!(baseline.contains_key(Path::new("domain.ttl")));
    assert_eq!(turtle(&baseline), turtle(&edited));
}

#[test]
fn failed_import_preserves_previous_publication_and_emits_replayable_diagnostic() {
    let scratch = tempfile::tempdir().unwrap();
    let src = scratch.path().join("ledger");
    let project = scratch.path().join("out");
    copy_fixture(&src);
    success(import(&src, &project));
    let baseline = files(&project);
    replace(
        &src,
        "src/lib.rs",
        "    Ok(())",
        "    state.clear();\n    Ok(())",
    );
    failure(import(&src, &project));
    let new_project = scratch.path().join("fresh");
    let msg = failure(import(&src, &new_project));
    assert!(msg.contains("src/lib.rs:"), "{msg}");
    assert_eq!(msg, failure(import(&src, &new_project)));
    assert!(!new_project.exists());
    assert_eq!(baseline, files(&project));
    success(verify(&project));
}

#[test]
fn stale_or_duplicated_identity_mappings_are_rejected() {
    let scratch = tempfile::tempdir().unwrap();
    let src = scratch.path().join("ledger");
    let project = scratch.path().join("out");
    for (from, to) in [
        ("urn:oddlang:ledger:g2", "urn:oddlang:ledger:g1"),
        ("SourceMissing =", "FormerName ="),
    ] {
        copy_fixture(&src);
        replace(&src, "mapping.toml", from, to);
        let msg = failure(import(&src, &project));
        assert!(msg.contains("identity") || msg.contains("variant"), "{msg}");
        assert!(!project.exists());
    }
}

#[test]
fn verification_and_build_reject_stale_generated_project() {
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    let native = scratch.path().join("native");
    success(import(&fixture(), &project));
    let domain = project.join("domain.ttl");
    let mut changed = fs::read(&domain).unwrap();
    changed.extend_from_slice(b"\n# after publication\n");
    fs::write(domain, changed).unwrap();
    failure(verify(&project));
    failure(cli(&["build".as_ref(), &project, &native]));
    assert!(!native.join("account.rs").exists());
}

#[test]
fn range_preconditions_and_snapshot_before_commit_are_required() {
    let scratch = tempfile::tempdir().unwrap();
    let src = scratch.path().join("ledger");
    let project = scratch.path().join("project");
    for (from, to) in [
        ("if balance < 0", "if balance < -1"),
        ("debit_before - amount_wide", "debit_before + amount_wide"),
        ("if source == destination", "if source != destination"),
        (
            "    state.get_mut(destination)",
            "    let late_read = i128::from(state[source].balance);\n    state.get_mut(destination)",
        ),
        (
            "return Err(TransferError::InsufficientFunds);",
            "state.get_mut(source).unwrap().balance = 0;\n        return Err(TransferError::InsufficientFunds);",
        ),
    ] {
        copy_fixture(&src);
        replace(&src, "src/lib.rs", from, to);
        let msg = failure(import(&src, &project));
        assert!(msg.contains("src/lib.rs:"), "{from}: {msg}");
        assert!(!project.exists());
    }
}

#[test]
fn invalid_overlay_aborts_staged_publication() {
    let scratch = tempfile::tempdir().unwrap();
    let src = scratch.path().join("ledger");
    let project = scratch.path().join("project");
    copy_fixture(&src);
    replace(&src, "overlays/fields.ttl", "o:min 0", "o:min 1");
    let msg = failure(import(&src, &project));
    assert!(msg.contains("bound") || msg.contains("range"), "{msg}");
    assert!(!project.exists());
    copy_fixture(&src);
    replace(
        &src,
        "overlays/avro.ttl",
        "l:Avro a o:Projection",
        "l:Avro a o:Operation",
    );
    failure(import(&src, &project));
    assert!(!project.exists());
}

#[test]
fn replay_validates_current_source_and_published_provenance() {
    let scratch = tempfile::tempdir().unwrap();
    let src = scratch.path().join("ledger");
    let project = scratch.path().join("project");
    copy_fixture(&src);
    success(import(&src, &project));
    let replay = || {
        cli(&[
            "import".as_ref(),
            "replay".as_ref(),
            &src.join("Cargo.toml"),
            &src.join("mapping.toml"),
            &project,
        ])
    };
    success(replay());
    replace(&src, "src/lib.rs", "credit_before", "previous_credit");
    failure(replay());
    copy_fixture(&src);
    fs::write(project.join("source-map.json"), "{}\n").unwrap();
    failure(replay());
}
