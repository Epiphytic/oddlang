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
        assert_eq!(
            vectors["transfer"]["error"]["error"],
            "urn:oddlang:ledger:error:InsufficientFunds"
        );
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
