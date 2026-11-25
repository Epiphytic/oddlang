//! Independent public CLI tests for recovery of an externally authored world.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const WORLD: &str = r#"package probe:recovery@1.0.0;
interface data {
    record %list {
        %type: s8,
        mid: s16,
        wide: s32,
        huge: s64,
    }
    variant choice {
        absent,
        present(list<option<%list>>),
    }
    inspect: func(%record: list<option<choice>>) -> result<%list, string>;
}
world demo {
    import seed: func() -> s64;
    export data;
}
"#;

fn input(root: &Path, wit: &str) -> PathBuf {
    let dir = root.join("input");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("world.wit"), wit).unwrap();
    dir
}

fn import(wit: &Path, identities: &str, out: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["wasm", "import-wit"])
        .arg(wit)
        .args(["demo", identities])
        .arg(out)
        .output()
        .unwrap()
}

fn success(result: Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn rejected(result: Output) -> String {
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(!error.contains("panicked"), "{error}");
    error
}

fn verify_no_artifacts(dir: &Path) {
    if dir.exists() {
        assert!(fs::read_dir(dir).unwrap().next().is_none());
    }
}

#[test]
fn raw_core_and_unsupported_valid_wit_fail_without_partial_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let out = root.join("recovered");
    let core = root.join("core.wasm");
    fs::write(&core, b"\0asm\x01\0\0\0").unwrap();
    rejected(import(&core, NAMESPACE, &out));
    verify_no_artifacts(&out);

    for wit in [
        "package probe:x@1.0.0; interface io { resource pipe; take: func(arg: borrow<pipe>); } world demo { export io; }",
        "package probe:x@1.0.0; world demo { export read: async func() -> s64; }",
        "package probe:x@1.0.0; world demo { export read: func() -> future<s64>; }",
    ] {
        let directory = input(root, wit);
        wit_parser::Resolve::default()
            .push_dir(&directory)
            .expect("well-formed WIT outside the synchronous profile");
        rejected(import(&directory, NAMESPACE, &out));
        verify_no_artifacts(&out);
    }
}

const NAMESPACE: &str = "urn:oddlang:import-probe:";

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn namespace_and_explicit_identities_recover_the_same_public_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wit = input(root, WORLD);
    let first = root.join("allocated");
    success(import(&wit, NAMESPACE, &first));

    let map = first.join("identities.json");
    let ids = read_json(&map);
    let declaration = &ids["interface/data/function/inspect"];
    assert!(declaration.as_str().unwrap().starts_with(NAMESPACE));
    assert_eq!(declaration, &ids["world/export/data/function/inspect"]);
    let missing = read_json(&first.join("missing-evidence.json"));
    for kind in ["behavior", "constraints"] {
        assert!(
            missing["missing"]
                .as_array()
                .unwrap()
                .contains(&Value::from(kind))
        );
    }

    let second = root.join("mapped");
    success(import(&wit, map.to_str().unwrap(), &second));
    for file in ["model.ttl", "identities.json", "missing-evidence.json"] {
        assert_eq!(
            fs::read(first.join(file)).unwrap(),
            fs::read(second.join(file)).unwrap(),
            "{file}"
        );
    }

    // Supply a separate behavioral witness before publishing the structural import.
    let corpus = serde_json::json!([{
        "caseId": "empty-probe",
        "covers": [
            {"declaration":declaration, "role":"wasm.exporter"},
            {"declaration":ids["world/import/seed"], "role":"wasm.importer"}
        ],
        "calls": [{"function":"data/inspect","arguments":[[]],"expected":{"err":"empty"}}],
        "imports": [{"function":"seed","arguments":[],"returns":"1"}]
    }]);
    fs::write(
        second.join("corpus.json"),
        serde_json::to_vec(&corpus).unwrap(),
    )
    .unwrap();
    fs::write(second.join("dependencies.json"), b"[]").unwrap();
    let release = root.join("release");
    success(
        Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(["wasm", "publish"])
            .arg(&second)
            .arg("contracts/wasm/empty.lock.json")
            .arg(root.join("cache"))
            .arg(&release)
            .output()
            .unwrap(),
    );

    let published = fs::read_to_string(release.join("wit/world.wit")).unwrap();
    let normalized = published.split_whitespace().collect::<String>();
    for surface in [
        "record%list{%type:s8,mid:s16,wide:s32,huge:s64,}",
        "variantchoice{absent,present(list<option<%list>>),}",
        "inspect:func(%record:list<option<choice>>)->result<%list,string>;",
        "importseed:func()->s64;",
        "exportdata;",
    ] {
        assert!(
            normalized.contains(surface),
            "missing {surface} in {published}"
        );
    }
    let third = root.join("roundtrip");
    success(import(
        &release.join("wit"),
        release.join("identities.json").to_str().unwrap(),
        &third,
    ));
    assert_eq!(
        fs::read(first.join("model.ttl")).unwrap(),
        fs::read(third.join("model.ttl")).unwrap()
    );
}

#[test]
fn incomplete_extra_and_conflicting_maps_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wit = input(root, WORLD);
    let first = root.join("valid");
    success(import(&wit, NAMESPACE, &first));
    let original = read_json(&first.join("identities.json"));
    for (path, value) in [
        ("world/import/seed", None),
        ("unknown", Some("urn:elsewhere")),
        ("world/export/data/function/inspect", Some("urn:divergent")),
        ("package", Some("bad iri")),
    ] {
        let mut ids = original.clone();
        let map = ids.as_object_mut().unwrap();
        match value {
            Some(id) => {
                map.insert(path.to_owned(), Value::from(id));
            }
            None => {
                map.remove(path).unwrap();
            }
        }
        let bad = root.join("bad.json");
        fs::write(&bad, serde_json::to_vec(map).unwrap()).unwrap();
        let out = root.join("failed");
        rejected(import(&wit, bad.to_str().unwrap(), &out));
        verify_no_artifacts(&out);
    }
}

#[test]
fn failure_preserves_preexisting_output_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wit = input(root, WORLD);
    let out = root.join("occupied");
    fs::create_dir_all(out.join("identities.json")).unwrap();
    fs::write(out.join("model.ttl"), b"reviewed model").unwrap();
    rejected(import(&wit, NAMESPACE, &out));
    assert_eq!(fs::read(out.join("model.ttl")).unwrap(), b"reviewed model");
    assert!(!out.join("missing-evidence.json").exists());
}

#[test]
fn explicit_map_cannot_collapse_distinct_parameters_with_different_types() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wit = input(
        root,
        "package probe:collision@1.0.0; world demo { export left: func(value: string); export right: func(value: s64); }",
    );
    let out = root.join("first");
    success(import(&wit, NAMESPACE, &out));
    let mut ids = read_json(&out.join("identities.json"));
    ids["world/export/right/param/value"] = ids["world/export/left/param/value"].clone();
    let bad = root.join("collapsed.json");
    fs::write(&bad, serde_json::to_vec(&ids).unwrap()).unwrap();
    let out = root.join("wrong");
    rejected(import(&wit, bad.to_str().unwrap(), &out));
    verify_no_artifacts(&out);
}

#[test]
fn foreign_types_recover_only_against_an_independent_release_pin() {
    use sha2::{Digest, Sha256};
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let echo_id = "urn:oddlang:echo:package";
    let key: String = Sha256::digest(echo_id)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let cache = root.join("cache");
    let echo = cache.join("wasm").join(key).join("1.0.0");
    let publish = |source: &Path, lock: &Path, dest: &Path| {
        success(
            Command::new(env!("CARGO_BIN_EXE_oddlang"))
                .args(["wasm", "publish"])
                .arg(source)
                .arg(lock)
                .arg(&cache)
                .arg(dest)
                .output()
                .unwrap(),
        );
    };
    publish(
        Path::new("contracts/wasm/echo"),
        Path::new("contracts/wasm/empty.lock.json"),
        &echo,
    );
    let source = root.join("consumer");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("model.ttl"),
        r#"
@prefix w: <urn:oddlang:wasm:> .
@prefix e: <urn:foreign:> .
e:p a w:Package ; w:name "probe:client@1.0.0" .
e:w a w:World ; w:name "demo" ; w:use e:m .
e:m a w:ForeignType ; w:name "message" ; w:from "sample:echo/shapes@1.0.0" .
"#,
    )
    .unwrap();
    fs::write(source.join("corpus.json"), "[]").unwrap();
    fs::write(
        source.join("dependencies.json"),
        serde_json::to_vec(&[echo_id]).unwrap(),
    )
    .unwrap();
    let release = root.join("bundle");
    publish(&source, &echo.join("workspace.lock.json"), &release);
    let independent = root.join("workspace.lock.json");
    fs::copy(release.join("workspace.lock.json"), &independent).unwrap();

    let wit = release.join("wit");
    let ids = release.join("identities.json");
    let out = root.join("recovered");
    let locked = |pin: &Path, map: &Path| {
        Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(["wasm", "import-wit"])
            .arg(&wit)
            .arg("demo")
            .arg(map)
            .arg(&out)
            .arg("--lock")
            .arg(pin)
            .output()
            .unwrap()
    };
    rejected(import(&wit, ids.to_str().unwrap(), &out));
    rejected(locked(&release.join("workspace.lock.json"), &ids));
    rejected(locked(&echo.join("workspace.lock.json"), &ids));
    let alternative = root.join("map.json");
    let mut map = read_json(&ids);
    map["world/use/message"] = Value::from("urn:other:message");
    fs::write(&alternative, serde_json::to_vec(&map).unwrap()).unwrap();
    rejected(locked(&independent, &alternative));
    success(locked(&independent, &ids));
    assert!(
        fs::read_to_string(out.join("model.ttl"))
            .unwrap()
            .contains("sample:echo/shapes@1.0.0")
    );
    fs::remove_dir_all(&out).unwrap();
    // Unlisted WIT never participates in resolution. A listed dependency must
    // match the externally pinned release even if its package identifier survives.
    fs::write(wit.join("decoy.wit"), "invalid WIT").unwrap();
    success(locked(&independent, &ids));
    fs::remove_dir_all(&out).unwrap();
    let dependency = fs::read_dir(wit.join("deps"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(dependency, "package sample:echo@1.0.0; interface shapes {}").unwrap();
    rejected(locked(&independent, &ids));
    verify_no_artifacts(&out);
}

#[test]
fn type_use_preserves_the_resolved_interfaces_callable_host_imports() {
    let tmp = tempfile::tempdir().unwrap();
    let wit = input(
        tmp.path(),
        r#"
package probe:uses@1.0.0;
interface data { record row { key: string } probe: func(); }
world demo { use data.{row}; export ping: func() -> row; }
"#,
    );
    let out = tmp.path().join("recovered");
    success(import(&wit, NAMESPACE, &out));
    let map = read_json(&out.join("identities.json"));
    let declaration = map["world/import/data/function/probe"].as_str().unwrap();
    assert_eq!(
        declaration,
        map["interface/data/function/probe"].as_str().unwrap()
    );
}
