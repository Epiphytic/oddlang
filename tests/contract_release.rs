use oddlang::contracts::resolve;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts")
}
fn project(path: &Path, version: &str) -> PathBuf {
    let file = path.join("project.ttl");
    fs::write(
        &file,
        format!(
            "<urn:project> <urn:oddlang:core:imports> <urn:oddlang:contract:ledger:{version}> ."
        ),
    )
    .unwrap();
    file
}
#[test]
fn published_closure_matches_source_and_loads_offline() {
    for version in ["v1", "v2"] {
        let tmp = tempdir().unwrap();
        let lock = root().join(format!("workspace-{version}.lock.json"));
        let r = resolve(&project(tmp.path(), version), &lock, &root().join("cache")).unwrap();
        assert_eq!(r.version, version);
        for entry in fs::read_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples/ledger")
                .join(version),
        )
        .unwrap()
        {
            let f = entry.unwrap();
            assert_eq!(
                r.payload(&format!("ontology/{}", f.file_name().to_str().unwrap()))
                    .unwrap(),
                fs::read(f.path()).unwrap()
            );
        }
        let vectors: serde_json::Value =
            serde_json::from_slice(r.payload("vectors.json").unwrap()).unwrap();
        assert_eq!(vectors["account"]["balance"], i64::MAX.to_string());
        if version == "v2" {
            assert!(vectors.get("transfer").is_none());
        } else {
            assert_eq!(
                vectors["transfer"]["error"]["error"],
                "urn:oddlang:ledger:error:InsufficientFunds"
            );
        }
        assert_eq!(
            vectors["account"].get("currencyCode").is_some(),
            version == "v2"
        );
        assert_eq!(r.model.fields.len(), if version == "v2" { 4 } else { 3 });
        assert!(
            resolve(
                &project(tmp.path(), "unknown"),
                &lock,
                &root().join("cache")
            )
            .is_err()
        );
    }
}
#[test]
fn cache_damage_is_named_before_loading() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let bundled = root().join("cache/ledger/v2");
    let dir = cache.join("ledger/v2");
    fs::create_dir_all(dir.join("ontology")).unwrap();
    for base in ["", "ontology"] {
        for f in fs::read_dir(bundled.join(base)).unwrap() {
            let f = f.unwrap();
            if f.path().is_file() {
                fs::copy(f.path(), dir.join(base).join(f.file_name())).unwrap();
            }
        }
    }
    let p = project(tmp.path(), "v2");
    let lock = root().join("workspace-v2.lock.json");
    for file in [
        "manifest.json",
        "wire.json",
        "vectors.json",
        "ontology/v1-to-v2.ttl",
    ] {
        let path = dir.join(file);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, b"changed").unwrap();
        let error = resolve(&p, &lock, &cache).err().unwrap();
        assert!(error.contains(file) && error.contains("digest"), "{error}");
        fs::remove_file(&path).unwrap();
        let error = resolve(&p, &lock, &cache).err().unwrap();
        assert!(error.contains(file) && error.contains("missing"), "{error}");
        fs::write(path, bytes).unwrap();
    }
}
#[test]
fn a_single_workspace_pin_upgrades_an_unchanged_importer() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("service.ttl");
    fs::write(
        &p,
        "<urn:service> <urn:oddlang:core:imports> <urn:oddlang:contract:ledger> .",
    )
    .unwrap();
    for (version, other) in [("v1", "v2"), ("v2", "v1")] {
        let lock = root().join(format!("workspace-{version}.lock.json"));
        let cache = root().join("cache");
        assert_eq!(resolve(&p, &lock, &cache).unwrap().version, version);
        let err = resolve(&project(tmp.path(), other), &lock, &cache)
            .err()
            .unwrap();
        assert!(err.contains("workspace pins"));
    }
}

#[test]
fn repinning_invalid_payloads_does_not_make_a_consistent_release() {
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    let hash = |b: &[u8]| {
        Sha256::digest(b)
            .iter()
            .map(|n| format!("{n:02x}"))
            .collect::<String>()
    };
    let src = root().join("cache/ledger/v1");
    for (mutation, needle) in [
        ("bad wire", "wire.json"),
        ("bad vectors", "vectors.json"),
        ("wrong fields", "accountFields"),
        ("wrong guards", "errorsByPriority"),
        ("wrong success type", "wire.json"),
        ("unreachable", "unreachable"),
        ("vector type", "balance"),
    ] {
        let tmp = tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let dir = cache.join("ledger/v1");
        fs::create_dir_all(dir.join("ontology")).unwrap();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(src.join("manifest.json")).unwrap()).unwrap();
        let files = manifest["files"].as_object_mut().unwrap();
        if mutation == "unreachable" {
            files.insert("ontology/stray.ttl".into(), json!(""));
        }
        for (name, digest) in files.iter_mut() {
            let mut bytes = fs::read(src.join(name)).unwrap_or_default();
            match (name.as_str(), mutation) {
                ("wire.json", "bad wire") => bytes = b"[]".to_vec(),
                ("vectors.json", "bad vectors") => bytes = b"not json".to_vec(),
                ("wire.json", "wrong fields")
                | ("wire.json", "wrong guards")
                | ("wire.json", "wrong success type")
                | ("vectors.json", "vector type") => {
                    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
                    if mutation == "wrong fields" {
                        v["accountFields"]["urn:bogus"] = json!("bogus");
                    }
                    if mutation == "wrong guards" {
                        v["transfer"]["errorsByPriority"]
                            .as_array_mut()
                            .unwrap()
                            .reverse();
                    }
                    if mutation == "wrong success type" {
                        v["transfer"]["success"] = json!(false);
                    }
                    if mutation == "vector type" {
                        v["account"]["balance"] = json!(42);
                    }
                    bytes = serde_json::to_vec(&v).unwrap();
                }
                _ => (),
            }
            *digest = json!(hash(&bytes));
            fs::write(dir.join(name), bytes).unwrap();
        }
        let bytes = serde_json::to_vec(&manifest).unwrap();
        fs::write(dir.join("manifest.json"), &bytes).unwrap();
        let lock = tmp.path().join("lock.json");
        fs::write(
            &lock,
            json!({"identity":manifest["identity"],"version":"v1","manifestSha256":hash(&bytes)})
                .to_string(),
        )
        .unwrap();
        let err = resolve(&project(tmp.path(), "v1"), &lock, &cache)
            .err()
            .expect(mutation);
        assert!(err.contains(needle), "{mutation}: {err}");
    }
}
