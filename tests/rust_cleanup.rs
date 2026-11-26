use std::{fs, path::Path, process::Command};

fn helper(args: &[&str], dir: &Path) -> std::process::Output {
    Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/skills/oddlang-import-cleanup/scripts/cleanup.py"
        ))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn proposals_require_digest_acceptance_and_keep_the_original() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    for root in ["old", "new"] {
        fs::create_dir(d.join(root)).unwrap();
        fs::write(d.join(root).join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(d.join(root).join("mapping.toml"), "guard = 'missing'\n").unwrap();
    }
    let original = "fn guard() { guard_missing!(); }\n";
    let effective = "fn guard() { if missing { return; } }\n";
    fs::write(d.join("old/lib.rs"), original).unwrap();
    fs::write(d.join("new/lib.rs"), effective).unwrap();
    assert!(
        helper(&["freeze", "old", "baseline.pin"], d)
            .status
            .success()
    );
    let proposal = helper(&["propose", "old", "new", "review"], d);
    assert!(
        proposal.status.success(),
        "{}",
        String::from_utf8_lossy(&proposal.stderr)
    );
    let digest = String::from_utf8(proposal.stdout).unwrap();
    assert!(!d.join("review/accepted.json").exists());
    assert!(
        !helper(&["accept", "review", "--sha256", "wrong"], d)
            .status
            .success()
    );
    let accepted = helper(&["accept", "review", "--sha256", digest.trim()], d);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        fs::read_to_string(d.join("review/original/lib.rs")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(d.join("review/effective/lib.rs")).unwrap(),
        effective
    );
    let verify = || {
        oddlang::import_rust::cleanup::verify(
            &d.join("review/accepted.json"),
            &d.join("review/effective/Cargo.toml"),
            &d.join("review/effective/mapping.toml"),
            &d.join("baseline.pin"),
        )
    };
    assert!(verify().is_ok(), "{:?}", verify());
    fs::write(d.join("review/original/lib.rs"), effective).unwrap();
    assert!(verify().is_err());
}

fn copy_fixture(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "target" {
            continue;
        }
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn observation(dir: &Path, target: &Path) -> String {
    let driver = target.with_extension("rs");
    fs::write(
        &driver,
        format!(
            r#"#[path = {:?}] mod ledger;
        fn main() {{
            let mut state = std::collections::BTreeMap::new();
            let error = ledger::transfer(&mut state, "s", "d", -1);
            println!("{{}}", match error {{
                Err(ledger::TransferError::SourceMissing) => "SourceMissing",
                Err(ledger::TransferError::NonPositiveAmount) => "NonPositiveAmount",
                _ => "unexpected"
            }});
        }}
    "#,
            dir.join("src/lib.rs")
        ),
    )
    .unwrap();
    assert!(
        Command::new("rustc")
            .args([
                "--edition",
                "2024",
                "-C",
                "overflow-checks=yes",
                "-Awarnings"
            ])
            .arg(driver)
            .arg("-o")
            .arg(target)
            .status()
            .unwrap()
            .success()
    );
    let result = Command::new(target).output().unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}

#[test]
fn review_macro_identity_and_silent_priority_changes_against_original() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/ledger");
    let code = fs::read_to_string(fixture.join("src/lib.rs")).unwrap();
    let src = "    if !state.contains_key(source) {\n        return Err(TransferError::SourceMissing);\n    }\n";
    let amount =
        "    if amount <= 0 {\n        return Err(TransferError::NonPositiveAmount);\n    }\n";
    assert!(code.contains(src) && code.contains(amount));
    for issue in ["macro", "identity", "priority"] {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let original = d.join("old");
        let edited = d.join("new");
        copy_fixture(&fixture, &original);
        copy_fixture(&fixture, &edited);
        match issue {
            "macro" => {
                let changed = format!(
                    "macro_rules! source_missing {{ ($state:ident, $source:ident) => {{ {} }} }}\n{}",
                    src.trim()
                        .replace("state.contains_key(source)", "$state.contains_key($source)"),
                    code.replace(src, "    source_missing!(state, source);\n")
                );
                fs::write(original.join("src/lib.rs"), changed).unwrap();
            }
            "identity" => {
                let map = fs::read_to_string(original.join("mapping.toml")).unwrap();
                fs::write(
                    original.join("mapping.toml"),
                    map.replace("urn:oddlang:ledger:g2", "urn:oddlang:ledger:g1"),
                )
                .unwrap();
            }
            "priority" => {
                fs::write(
                    edited.join("src/lib.rs"),
                    code.replace(amount, "")
                        .replace(src, &format!("{amount}{src}")),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let pin = d.join("baseline.pin");
        let import = |root: &Path, out: &str, receipt: Option<&Path>| {
            oddlang::import_rust::import(
                &root.join("Cargo.toml"),
                &root.join("mapping.toml"),
                &d.join(out),
                receipt.map(|r| (r, pin.as_path())),
            )
        };
        if issue != "priority" {
            assert!(import(&original, "before", None).is_err());
        }
        assert!(
            helper(&["freeze", "old", "baseline.pin"], d)
                .status
                .success()
        );
        let proposal = helper(&["propose", "old", "new", "review"], d);
        assert!(
            proposal.status.success(),
            "{}",
            String::from_utf8_lossy(&proposal.stderr)
        );
        let digest = String::from_utf8(proposal.stdout).unwrap();
        assert!(
            helper(&["accept", "review", "--sha256", digest.trim()], d)
                .status
                .success()
        );
        let effective = d.join("review/effective");
        let receipt = d.join("review/accepted.json");
        import(&effective, "project", Some(&receipt)).unwrap();
        oddlang::import_rust::replay(
            &effective.join("Cargo.toml"),
            &effective.join("mapping.toml"),
            &d.join("project"),
            Some((&receipt, &pin)),
        )
        .unwrap();
        let baseline = observation(&d.join("review/original"), &d.join("baseline"));
        let candidate = observation(&effective, &d.join("candidate"));
        assert_eq!(baseline, "SourceMissing");
        assert_eq!(
            candidate,
            if issue == "priority" {
                "NonPositiveAmount"
            } else {
                "SourceMissing"
            }
        );
    }
}

#[test]
fn accepted_replacement_archive_cannot_substitute_the_pre_cleanup_baseline() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/ledger");
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    copy_fixture(&fixture, &d.join("old"));
    copy_fixture(&fixture, &d.join("new"));
    assert!(
        helper(&["freeze", "old", "trusted.pin"], d)
            .status
            .success()
    );
    let source = d.join("old/src/lib.rs");
    let code = fs::read_to_string(&source).unwrap();
    let a = code.find("    if !state.contains_key(source)").unwrap();
    let b = code
        .find("    if !state.contains_key(destination)")
        .unwrap();
    let c = code.find("    if source == destination").unwrap();
    fs::write(
        &source,
        format!("{}{}{}{}", &code[..a], &code[b..c], &code[a..b], &code[c..]),
    )
    .unwrap();

    let proposed = helper(&["propose", "old", "new", "review"], d);
    assert!(proposed.status.success());
    let digest = String::from_utf8(proposed.stdout).unwrap();
    assert!(
        helper(&["accept", "review", "--sha256", digest.trim()], d)
            .status
            .success()
    );
    let import = |pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .current_dir(d)
            .args([
                "import",
                "rust",
                "review/effective/Cargo.toml",
                "review/effective/mapping.toml",
                "out",
                "--accepted-cleanup",
                "review/accepted.json",
                "--baseline-pin",
                pin,
            ])
            .output()
            .unwrap()
    };
    let error = |pin: &str| {
        let result = import(pin);
        assert!(!result.status.success());
        assert!(!d.join("out").exists());
        String::from_utf8(result.stderr).unwrap()
    };
    assert!(error("trusted.pin").contains("independently frozen baseline"));
    assert!(error("missing.pin").contains("missing independent baseline pin"));
    fs::copy(d.join("trusted.pin"), d.join("review/local.pin")).unwrap();
    assert!(error("review/local.pin").contains("outside the cleanup archive"));
    fs::write(d.join("review/accepted.json"), "{}").unwrap();
    assert!(error("trusted.pin").contains("differs from reviewed proposal"));
}
