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
        let parsed: serde_json::Value = serde_json::from_slice(&lock).unwrap();
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
        let current = parsed["files"][name].as_str().unwrap();
        let changed = std::str::from_utf8(&lock).unwrap().replace(current, &sha);
        std::fs::write(&path, payload).unwrap();
        std::fs::write(&lock_path, changed).unwrap();
        assert!(load_verified(&output).is_err(), "{name}");
        std::fs::write(path, original).unwrap();
        std::fs::write(lock_path, lock).unwrap();
    }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn provenance_checks_each_graph_fact_and_pinned_import_without_the_source() {
    use oddlang::framing_import::{load_pinned, verify_pinned};
    use std::fs;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fixture(&root);
    let frame = root.join("src/frame.rs");
    let src = fs::read_to_string(&frame)
        .unwrap()
        .replace("pub const RAW_BYTES", "/* 雪 */ pub const RAW_BYTES");
    fs::write(&frame, &src).unwrap();
    let out = temp.path().join("original");
    import(&root.join("Cargo.toml"), &root.join("mapping.toml"), &out).unwrap();
    let graph_path = out.join("domain.ttl");
    let lock_path = out.join("import.lock");
    let lock = fs::read_to_string(&lock_path).unwrap();
    let pin = digest(lock.as_bytes());
    let map: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("source-map.json")).unwrap()).unwrap();
    assert_eq!(map["fileSha256"], digest(src.as_bytes()));
    for (id, c) in map["locations"].as_object().unwrap() {
        let start = c["byte_start"].as_u64().unwrap() as usize;
        let end = c["byte_end"].as_u64().unwrap() as usize;
        assert_eq!(map["spanSha256"][id], digest(&src.as_bytes()[start..end]));
        if id == "Limits.RAW_BYTES" {
            assert_eq!(&src[start..end], "pub const RAW_BYTES: usize = 7;");
        }
    }
    let graph = fs::read_to_string(&graph_path).unwrap();
    assert_eq!(
        map["facts"].as_object().unwrap().len(),
        graph.lines().count()
    );
    for statement in graph.lines() {
        let sites = map["facts"][digest(statement.as_bytes())]["sites"]
            .as_array()
            .unwrap();
        assert!(!sites.is_empty());
        if statement.starts_with("<urn:oddlang:framing:version> <urn:oddlang:framing:value>") {
            assert_eq!(sites, &["encode.json-encode-base64", "decode.version"]);
        }
    }
    let changed = graph.replace(
        "framing:RAW_BYTES> <urn:oddlang:framing:value> \"7\"",
        "framing:RAW_BYTES> <urn:oddlang:framing:value> \"6\"",
    );
    assert_ne!(graph, changed);
    fs::write(&graph_path, &changed).unwrap();
    fs::write(
        &lock_path,
        lock.replace(&digest(graph.as_bytes()), &digest(changed.as_bytes())),
    )
    .unwrap();
    assert!(
        load_verified(&out)
            .unwrap_err()
            .contains("source-span bindings")
    );
    assert!(
        load_pinned(&out, &pin)
            .unwrap_err()
            .contains("external framing import pin mismatch")
    );
    fs::write(graph_path, &graph).unwrap();
    fs::write(lock_path, &lock).unwrap();

    // A different valid import has fresh graph, source-span and build hashes.
    // The original independently retained pin is the distinguishing input.
    fs::write(
        &frame,
        src.replace("RAW_BYTES: usize = 7;", "RAW_BYTES: usize = 6;"),
    )
    .unwrap();
    let rewritten = temp.path().join("rewritten");
    import(
        &root.join("Cargo.toml"),
        &root.join("mapping.toml"),
        &rewritten,
    )
    .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert_eq!(load_verified(&rewritten).unwrap().0.raw_limit, 6);
    assert_eq!(load_pinned(&out, &pin).unwrap().0.raw_limit, 7);
    assert!(
        verify_pinned(&rewritten, &pin)
            .unwrap_err()
            .contains("external framing import pin mismatch")
    );
}
