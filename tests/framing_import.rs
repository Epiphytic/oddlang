use oddlang::framing_import::{import, load_verified};
use std::path::Path;

#[test]
fn independent_project_records_distinct_limits_and_error_order() {
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/framing-independent");
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("result");
    import(&src.join("Cargo.toml"), &src.join("mapping.toml"), &dir).unwrap();
    let (f, hash) = load_verified(&dir).unwrap();
    assert_eq!(
        (f.raw_limit, f.json_limit, f.ndjson_limit, f.version),
        (7, 128, 256, 7)
    );
    assert_eq!(&f.decode_steps[2..4], ["encoded-limit", "version"]);
    assert_eq!(hash.len(), 64);
}

fn fixture(dir: &Path) {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/rust-import/framing-independent");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    for p in [
        "Cargo.toml",
        "Cargo.lock",
        "mapping.toml",
        "src/lib.rs",
        "src/frame.rs",
    ] {
        std::fs::copy(root.join(p), dir.join(p)).unwrap();
    }
}

#[test]
fn unsupported_asts_are_rejected_deterministically_before_publication() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let source = std::fs::read_to_string(root.join("src/frame.rs")).unwrap();
    for (from, to) in [
        (
            "use thiserror::Error;",
            "use thiserror::Error;\nmod serde_json {}",
        ),
        (
            "#[serde(deny_unknown_fields)]",
            "#[serde(default, deny_unknown_fields)]",
        ),
        ("STANDARD.encode(content)", "STANDARD.encode(&[])"),
        (
            "map_err(|_| FrameError::InvalidJson)",
            "map_err(|_| { FrameError::InvalidEncoding })",
        ),
        (
            "pub struct ProbeEnvelope;",
            "#[cfg(unix)] pub struct ProbeEnvelope;",
        ),
        (
            "pub struct ProbeEnvelope;",
            "#[doc = include_str!(\"lib.rs\")] pub struct ProbeEnvelope;",
        ),
        (
            "pub const RAW_BYTES: usize = 7;",
            "pub const RAW_BYTES: usize = 4294967295;",
        ),
    ] {
        assert!(source.contains(from));
        std::fs::write(root.join("src/frame.rs"), source.replace(from, to)).unwrap();
        let attempt = || {
            import(
                &root.join("Cargo.toml"),
                &root.join("mapping.toml"),
                &root.join("out"),
            )
            .unwrap_err()
        };
        assert_eq!(attempt(), attempt());
        assert!(!root.join("out").exists());
    }
}

#[test]
fn altered_dependency_features_and_cargo_cfg_fail_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let manifest = root.join("Cargo.toml");
    let original = std::fs::read_to_string(&manifest).unwrap();
    let altered = original.replace(
        "base64 = \"=0.22.1\"",
        "base64 = {version=\"=0.22.1\", default-features=false}",
    );
    assert_ne!(original, altered);
    std::fs::write(&manifest, altered).unwrap();
    let attempt = || import(&manifest, &root.join("mapping.toml"), &root.join("out")).unwrap_err();
    assert!(attempt().contains("features"));
    std::fs::write(&manifest, original).unwrap();
    std::fs::create_dir(root.join(".cargo")).unwrap();
    std::fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustflags=['--cfg','rewritten']\n",
    )
    .unwrap();
    assert!(attempt().contains("local Cargo configuration"));
    assert!(!root.join("out").exists());
}

#[test]
fn raw_text_in_graph_or_coordinates_is_rejected_even_when_rehashed() {
    use sha2::{Digest, Sha256};
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let output = root.join("out");
    import(
        &root.join("Cargo.toml"),
        &root.join("mapping.toml"),
        &output,
    )
    .unwrap();
    for name in ["domain.ttl", "source-map.json"] {
        let path = output.join(name);
        let lock_path = output.join("import.lock");
        let original = std::fs::read_to_string(&path).unwrap();
        let lock = std::fs::read(&lock_path).unwrap();
        let mut changed: serde_json::Value = serde_json::from_slice(&lock).unwrap();
        let payload = if name == "domain.ttl" {
            format!(
                "{original}<urn:oddlang:framing:codec> <urn:oddlang:framing:source> \"pub fn inserted() {{}}\" .\n"
            )
        } else {
            original.replacen(
                "\"line\":",
                "\"implementation\": \"fn inserted() {}\", \"line\":",
                1,
            )
        };
        let sha = Sha256::digest(payload.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        changed["files"][name] = sha.clone().into();
        if name == "domain.ttl" {
            changed["graphSha256"] = sha.into();
        }
        std::fs::write(&path, payload).unwrap();
        std::fs::write(&lock_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(load_verified(&output).is_err(), "{name}");
        std::fs::write(path, original).unwrap();
        std::fs::write(lock_path, lock).unwrap();
    }
}
