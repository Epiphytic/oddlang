//! One consumer and one independently authored contract apply to both implementations.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn oddlang() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
}
fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/providers")
}
fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn consumer(lib: &Path, work: &Path) {
    let rlib = work.join("librecords.rlib");
    run(Command::new("rustc")
        .args([
            "--crate-type",
            "rlib",
            "--crate-name",
            "records",
            "--edition",
            "2021",
        ])
        .arg(lib)
        .arg("-o")
        .arg(&rlib));
    let test = work.join("consumer");
    run(Command::new("rustc")
        .arg(example().join("fixture/consumer.rs"))
        .arg("--extern")
        .arg(format!("records={}", rlib.display()))
        .arg("-o")
        .arg(&test));
    run(&mut Command::new(test));
}

#[test]
fn same_native_consumer_and_portable_recovery_for_both_providers() {
    let root = example();
    let work = tempfile::tempdir().unwrap();
    for (name, registry) in [
        ("native", "native-registry.json"),
        ("wasm", "registry.json"),
    ] {
        let dir = work.path().join(name);
        fs::create_dir(&dir).unwrap();
        let recovered = dir.join("recovered");
        run(oddlang()
            .current_dir(&root)
            .args([
                "recover",
                "public-root-records",
                "fixture/source.rs",
                "fixture/selection.json",
            ])
            .arg(&recovered)
            .args(["--providers", registry]));
        for file in ["graph.json", "rust.json"] {
            assert_eq!(
                json(&recovered.join(file)),
                json(&root.join("fixture").join(file))
            );
        }
        let receipt = json(&recovered.join("provider-receipt.json"));
        assert_eq!(receipt["role"], "recover");
        assert_eq!(receipt["evidence"], serde_json::json!(["source.rs"]));
        assert_eq!(
            receipt["provider"]["componentSha256"].is_string(),
            name == "wasm"
        );
        let projected = dir.join("projected");
        run(oddlang()
            .current_dir(&root)
            .args(["build", "public-records", "rust"])
            .arg(recovered.join("graph.json"))
            .arg(recovered.join("rust.json"))
            .arg(&projected)
            .args(["--providers", registry]));
        if name == "wasm" {
            assert_eq!(receipt, json(&root.join("receipts/recover.json")));
            assert_eq!(
                json(&projected.join("provider-receipt.json")),
                json(&root.join("receipts/generate.json"))
            );
        }
        consumer(&projected.join("records.rs"), &dir);
        let workspace = dir.join("workspace");
        run(oddlang()
            .current_dir(&root)
            .args(["build", "native-workspace", "rust"])
            .arg(format!("workspace-{name}.json"))
            .arg(&workspace));
        consumer(&workspace.join("records/src/lib.rs"), &dir);
        let receipt = workspace.join("records/provider-receipt.json");
        if name == "wasm" {
            let receipt = json(&receipt);
            let registered = json(&root.join("registry.json"));
            assert_eq!(
                receipt["components"]["0"]["provider"]["componentSha256"],
                registered["providers"][0]["component"]["sha256"]
            );
        } else {
            assert!(!receipt.exists());
        }
    }
}

#[test]
fn unknown_capability_and_selected_library_binding_use_the_same_host() {
    let work = tempfile::tempdir().unwrap();
    let root = example();
    for (cap, input) in [
        ("example.field-list", "fields"),
        ("example.library-binding", "binding"),
    ] {
        let dir = work.path().join(input);
        run(oddlang()
            .current_dir(&root)
            .args(["provider", "run", "generate", cap, "1", "registry.json"])
            .arg(format!("fixture/{input}-input.json"))
            .arg(&dir));
        assert_eq!(
            json(&dir.join("provider-receipt.json")),
            json(&root.join(format!("receipts/{input}.json")))
        );
        if input == "fields" {
            assert_eq!(
                fs::read(dir.join("fields.txt")).unwrap(),
                fs::read(root.join("fixture/expected-fields.txt")).unwrap()
            );
        } else {
            assert_eq!(
                json(&dir.join("binding.json")),
                json(&root.join("fixture/expected-binding.json"))
            );
        }
    }
}

#[test]
fn library_binding_rejects_ids_outside_its_declared_namespace() {
    let root = example();
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("input.json");
    fs::write(&input, r#"{"id":"urn:other:duration"}"#).unwrap();
    let destination = work.path().join("output");
    let result = oddlang()
        .current_dir(&root)
        .args([
            "provider",
            "run",
            "generate",
            "example.library-binding",
            "1",
            "registry.json",
        ])
        .arg(input)
        .arg(&destination)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("namespace")
    );
    assert!(!destination.exists());
}

#[test]
fn workspace_rejects_registry_configuration_drift_before_projection() {
    let root = example();
    let work = tempfile::tempdir().unwrap();
    let mut registry = json(&root.join("registry.json"));
    registry["providers"][0]["configuration"] = serde_json::json!({"changed": true});
    fs::write(
        work.path().join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    fs::copy(
        root.join("workspace-wasm.json"),
        work.path().join("workspace.json"),
    )
    .unwrap();
    let result = oddlang()
        .current_dir(work.path())
        .args([
            "build",
            "native-workspace",
            "rust",
            "workspace.json",
            "output",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let diagnostic = String::from_utf8(result.stderr).unwrap();
    assert!(
        diagnostic.contains("digest") && diagnostic.contains("registry"),
        "{diagnostic}"
    );
    assert!(!work.path().join("output").exists());
}
