//! An independently authored guest can supply a Rust module to a workspace.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn pin(root: &Path, name: &str, bytes: &[u8]) -> Value {
    fs::write(root.join(name), bytes).unwrap();
    json!({"path":name,"sha256":Sha256::digest(bytes).iter().map(|b|format!("{b:02x}")).collect::<String>()})
}

#[test]
fn unknown_capability_composes_as_a_rust_library() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = b"pub fn station_id() -> u8 { 37 }";
    let response = json!({"format":"oddlang-provider-output-v1",
        "artifacts":[{"path":"module.rs","base64":STANDARD.encode(source)}],
        "exports":["station_id"],"dependencies":[],"evidence":[],"gaps":[]})
    .to_string();
    let data = response
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let wat = format!(
        r#"(component (core module $m (memory (export "mem") 1)
        (data (i32.const 16) "{data}")
        (func (export "alloc") (param i32 i32 i32 i32) (result i32) i32.const 4096)
        (func (export "run") (param i32 i32) (result i32)
            i32.const 0 i32.const 16 i32.store i32.const 4 i32.const {len} i32.store i32.const 0))
        (core instance $i (instantiate $m))
        (func (export "generate") (param "request" (list u8)) (result (list u8))
            (canon lift (core func $i "run") (memory $i "mem") (realloc (func $i "alloc")))))"#,
        len = response.len()
    );
    let component = pin(root, "fixture.wasm", &wat::parse_str(wat).unwrap());
    let schema=pin(root,"schema.json",br#"{"type":"object","properties":{"graph":{"type":"object"},"overlay":{"type":"object"}},"required":["graph","overlay"],"additionalProperties":false}"#);
    let registry = pin(root,"registry.json",json!({"format":"oddlang-provider-registry-v1",
        "providers":[{"role":"generate","capability":"example.fixture","version":"1","world":"oddlang:provider/generate@1.0.0","configuration":{},"component":component,"schema":schema}]}).to_string().as_bytes());
    let cargo = pin(
        root,
        "Cargo.lock",
        b"version = 4\n\n[[package]]\nname = \"station\"\nversion = \"0.1.0\"\n",
    );
    let graph = pin(root, "graph.json", b"{}");
    let package=pin(root,"package.json",json!({"format":"oddlang-native-package-v1","name":"station","version":"0.1.0","edition":"2024","cargoLock":cargo,
        "components":[{"kind":"provider","provider":{"capability":"example.fixture","version":"1"},"module":"measurements","graph":graph,"overlay":graph,"reexports":["station_id"]}]}).to_string().as_bytes());
    let manifest = root.join("workspace.json");
    fs::write(&manifest,json!({"format":"oddlang-native-workspace-v1","cargoLock":cargo,"providerRegistry":registry,"members":{"station":{"manifest":package}}}).to_string()).unwrap();
    let output = root.join("out");
    oddlang::native_workspace::build(&manifest, &output).unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(output.join("station/provider-receipt.json")).unwrap())
            .unwrap();
    assert_eq!(
        receipt["components"]["0"]["provider"]["componentSha256"],
        component["sha256"]
    );
    let test_dir = output.join("station/tests");
    fs::create_dir(&test_dir).unwrap();
    fs::write(
        test_dir.join("consumer.rs"),
        "#[test] fn station() { assert_eq!(station::station_id(), 37); }",
    )
    .unwrap();
    assert!(
        Command::new("cargo")
            .args([
                "test",
                "--locked",
                "--offline",
                "--quiet",
                "--test",
                "consumer"
            ])
            .current_dir(output)
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap()
            .success()
    );
}
