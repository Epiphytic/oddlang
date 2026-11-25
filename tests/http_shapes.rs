use oddlang::http::{
    CaseResult, Claim, Evidence, load_source, load_workspace, publish, publish_successor,
    read_json, verify_bundle, verify_evidence,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const FIXTURE: &str = "examples/http/contract/shapes/v1";
const CACHE: &str = "contracts/cache";
const SHAPE_RELEASE: &str =
    "contracts/cache/http/d550a36a22266cc54b0f99128f2851810bac7a782e5bdd519239f260a1f7392e/v1";
const SHAPES: &str = "urn:oddlang:contract:shapes-http";
const LEDGER: &str = "urn:oddlang:contract:ledger-http";

fn run(command: &mut Command) -> Vec<u8> {
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("{command:?}: {err}"));
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn observe(command: &mut Command, input: &[u8]) -> Vec<CaseResult> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // Feed the same verified portable corpus to each native runner.
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{command:?}: {:?}", out.status);
    serde_json::from_slice(&out.stdout).unwrap()
}

fn source(r: &Path) -> oddlang::http::HttpContract {
    load_source(r, &r.join("dependencies.lock.json"), Path::new(CACHE)).unwrap()
}

#[test]
fn codec_record_requires_closed_declaration() {
    let temp = tempfile::tempdir().unwrap();
    let domain = fs::read_to_string(Path::new(FIXTURE).join("domain.ttl")).unwrap();
    let record = "s:Metadata a h:Record ; h:field s:EnabledField ; h:constraint s:Closed .";
    assert!(domain.contains(record));
    let invalid = domain.replace(record, "s:Metadata a h:Record ; h:field s:EnabledField .");
    fs::write(temp.path().join("domain.ttl"), invalid).unwrap();
    fs::write(temp.path().join("dependencies.lock.json"), r#"{"pins":[]}"#).unwrap();
    let error = load_source(
        temp.path(),
        &temp.path().join("dependencies.lock.json"),
        Path::new(CACHE),
    )
    .unwrap_err();
    assert!(
        error.contains("urn:oddlang:shapes:Metadata urn:oddlang:http:constraint: expected Closed"),
        "{error}"
    );
}

#[test]
fn three_native_codecs_round_trip_the_same_pinned_cases() {
    let root = Path::new(FIXTURE);
    let pin = root.join("workspace.lock.json");
    let temp = tempfile::tempdir().unwrap();
    let published = temp.path().join("published");
    publish(&source(root), &published).unwrap();
    for entry in fs::read_dir(SHAPE_RELEASE).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(
            fs::read(entry.path()).unwrap(),
            fs::read(published.join(entry.file_name())).unwrap()
        );
    }
    let b = verify_bundle(&published, &pin).unwrap();
    assert_eq!(b.rule_set_version, 2);
    let claim: Claim = read_json(&root.join("claim.json")).unwrap();
    for (position, kind, expected) in [
        ("body/choice", "unknown-tag", "localRejection"),
        ("body/note", "omitted", "localRejection"),
        ("body/memo", "null", "localRejection"),
        ("body/items", "invalid-element", "localRejection"),
        ("body/note", "null", "success"),
        ("body/memo", "omitted", "success"),
        ("body/items", "empty", "success"),
        ("body", "bom", "localRejection"),
        ("body", "duplicate-decoded-member", "localRejection"),
        ("body/note", "isolated-surrogate", "localRejection"),
        ("body/note", "invalid-utf8", "localRejection"),
        ("body/items/0", "number-token", "localRejection"),
    ] {
        let witnesses: Vec<_> = b
            .cases
            .iter()
            .filter(|case| {
                case.slots
                    .iter()
                    .any(|s| s.position == position && s.kind == kind)
            })
            .collect();
        assert!(
            !witnesses.is_empty(),
            "{position}/{kind} must have a witness"
        );
        assert!(
            witnesses
                .iter()
                .all(|case| case.expected[0]["native"]["kind"] == expected)
        );
    }

    let generated = temp.path().join("generated");
    run(Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["http", "generate"])
        .args([
            root.join("project.ttl"),
            pin,
            PathBuf::from(CACHE),
            root.join("overlay.json"),
            generated.clone(),
        ]));
    let rust_target = fs::canonicalize("target")
        .unwrap()
        .join("http-shape-native");
    run(Command::new(env!("CARGO"))
        .args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(root.join("rust-runner/Cargo.toml"))
        .env("ODDLANG_SHAPE_CODEC", generated.join("codec.rs"))
        .env("CARGO_TARGET_DIR", &rust_target));
    run(Command::new("target/http-tools/node_modules/.bin/tsc")
        .args(["--strict", "--target", "ES2022", "--module", "commonjs"])
        .arg(generated.join("codec.ts")));
    let go_runner = temp.path().join("codec-go");
    run(Command::new("go")
        .current_dir(root.join("go-runner"))
        .args(["build", "-o"])
        .arg(&go_runner)
        .arg(".")
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .env("GOPROXY", "off"));
    let bytes = fs::read(published.join("cases.json")).unwrap();
    let output = Path::new("target/http-conformance");
    fs::create_dir_all(output).unwrap();
    for (language, results) in [
        (
            "rust",
            observe(
                &mut Command::new(rust_target.join("debug/oddlang-shape-witness")),
                &bytes,
            ),
        ),
        (
            "typescript",
            observe(
                Command::new("node")
                    .arg(root.join("typescript-runner.cjs"))
                    .arg(generated.join("codec.js")),
                &bytes,
            ),
        ),
        ("go", observe(&mut Command::new(go_runner), &bytes)),
    ] {
        let evidence = Evidence {
            schema_version: 1,
            release: b.pin.clone(),
            rule_set_version: b.rule_set_version,
            claims: claim.claims.clone(),
            results,
        };
        verify_evidence(&b, &claim, &evidence).unwrap_or_else(|err| panic!("{language}: {err}"));
        fs::write(
            output.join(format!("shapes-{language}.json")),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn real_codec_and_ledger_imports_keep_original_ledger_pins() {
    let root = Path::new(FIXTURE);
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project.ttl");
    fs::write(
        &project,
        format!("<urn:example:shared> <urn:oddlang:core:imports> <{LEDGER}>, <{SHAPES}> ."),
    )
    .unwrap();
    let shape_lock: Value = read_json(&root.join("workspace.lock.json")).unwrap();
    let lock = temp.path().join("workspace.lock.json");
    for (version, previous, digest) in [
        (
            "v1",
            None,
            "e1cf1857145ff54c538de9f14938fae607e1e925a5220dcc9feafb92aa60266a",
        ),
        (
            "v2",
            Some("v1"),
            "0922d46293a8bb9f8a5221dffa1fd5a87850299efdc99efe14ad659163276b0e",
        ),
    ] {
        let ledger_root = Path::new("examples/http/contract").join(version);
        let ledger_lock: Value = read_json(&ledger_root.join("workspace.lock.json")).unwrap();
        fs::write(
            &lock,
            serde_json::to_vec(&json!({"pins":[ledger_lock["pins"][0],shape_lock["pins"][0]]}))
                .unwrap(),
        )
        .unwrap();
        let models = load_workspace(&project, &lock, Path::new(CACHE)).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[SHAPES].rule_set_version, 2);
        assert_eq!(models[LEDGER].version, version);

        let out = temp.path().join(version);
        let model = source(&ledger_root);
        if let Some(previous) = previous {
            let predecessor = Path::new("examples/http/contract")
                .join(previous)
                .join("workspace.lock.json");
            publish_successor(&model, &predecessor, Path::new(CACHE), &out).unwrap();
        } else {
            publish(&model, &out).unwrap();
        }
        let bundle = verify_bundle(&out, &lock).unwrap();
        assert_eq!(bundle.pin.manifest_sha256, digest);
    }
}
