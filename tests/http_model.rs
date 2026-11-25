use oddlang::http::{load, load_source};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn demo() -> PathBuf {
    PathBuf::from("examples/http/contract/v1")
}
fn cache() -> &'static Path {
    Path::new("contracts/cache")
}
fn check(source: &str) -> Result<oddlang::http::HttpContract, oddlang::http::HttpError> {
    let dir = tempdir().unwrap();
    fs::copy(
        demo().join("scenarios.json"),
        dir.path().join("scenarios.json"),
    )
    .unwrap();
    fs::write(dir.path().join("domain.ttl"), source).unwrap();
    load_source(dir.path(), &demo().join("dependencies.lock.json"), cache())
}
fn source() -> String {
    fs::read_to_string(demo().join("domain.ttl")).unwrap()
}
#[test]
fn published_matches_source() {
    let source = check(&source()).unwrap();
    let release = load(
        &demo().join("project.ttl"),
        &demo().join("workspace.lock.json"),
        cache(),
    )
    .unwrap();
    assert_eq!(source, release);
    assert_eq!(source.operations.len(), 2);
}
#[test]
fn malformed_declarations() {
    for (old, new) in [
        ("h:wireName \"balance\"", "h:typo \"balance\""),
        ("h:wireName \"balance\" ;", ""),
        ("h:response d:Account", "h:response d:Absent"),
    ] {
        let original = source();
        assert!(original.contains(old));
        assert!(check(&original.replace(old, new)).is_err());
    }
}
#[test]
fn cyclic_imports_and_blank_metadata() {
    let dir = tempdir().unwrap();
    fs::copy(
        demo().join("scenarios.json"),
        dir.path().join("scenarios.json"),
    )
    .unwrap();
    let mut original = source();
    original.push_str("\nd:Contract h:localImport \"other.ttl\" .\n");
    fs::write(dir.path().join("domain.ttl"), original).unwrap();
    fs::write(dir.path().join("other.ttl"), "@prefix h: <urn:oddlang:http:> . <urn:oddlang:ledger-http:Contract> h:localImport \"domain.ttl\" .").unwrap();
    assert_eq!(
        load_source(dir.path(), &demo().join("dependencies.lock.json"), cache())
            .unwrap()
            .operations,
        check(&source()).unwrap().operations
    );
    let first = format!(
        "{}\n_:a <http://www.w3.org/2000/01/rdf-schema#label> \"example\" .",
        source()
    );
    let second = format!(
        "_:b <http://www.w3.org/2000/01/rdf-schema#label> \"example\" .\n{}",
        source()
    );
    let mut a = check(&first).unwrap();
    let mut b = check(&second).unwrap();
    a.source_files.clear();
    b.source_files.clear();
    assert_eq!(a, b);
}

#[test]
fn orphan_declarations_and_recursive_records_fail() {
    assert!(
        check(&(source() + "\nd:Unused a h:Required ."))
            .unwrap_err()
            .contains("unreachable")
    );
    assert!(
        check(&source().replace(
            "d:Account a h:Record ; h:field d:id",
            "d:Account a h:Record ; h:field d:sourceAccount, d:id"
        ))
        .unwrap_err()
        .contains("cyclic")
    );
}

#[test]
fn conflicting_pins_fail_before_resolution() {
    let dir = tempdir().unwrap();
    let lock = dir.path().join("lock.json");
    let mut data: serde_json::Value =
        serde_json::from_slice(&fs::read(demo().join("dependencies.lock.json")).unwrap()).unwrap();
    let mut other = data["pins"][0].clone();
    other["version"] = "v2".into();
    data["pins"].as_array_mut().unwrap().push(other);
    fs::write(&lock, data.to_string()).unwrap();
    assert!(
        load_source(&demo(), &lock, cache())
            .unwrap_err()
            .contains("conflicting pins")
    );
}

#[test]
fn changed_pinned_file_rejected_before_parse() {
    let f = Published::copy();
    let file = f.release.join("domain.ttl");
    let mut bytes = fs::read(&file).unwrap();
    bytes[0] ^= 1;
    fs::write(file, bytes).unwrap();
    assert!(f.error().contains("digest mismatch for domain.ttl"));
}

#[test]
fn duplicate_route_names_the_operation() {
    let duplicate = source()
        .replace(
            "h:method \"POST\" ; h:path \"/transfers\"",
            "h:method \"GET\" ; h:path \"/accounts/{id}\"",
        )
        .replace("h:request d:Transfer", "h:request d:Lookup");
    assert!(check(&duplicate).unwrap_err().contains("duplicate route"));
}

#[cfg(unix)]
#[test]
fn source_paths_are_normal_and_contained() {
    for path in [
        "../other.ttl",
        "./other.ttl",
        "parts//other.ttl",
        "/other.ttl",
    ] {
        assert!(
            check(&(source() + &format!("\nd:Contract h:localImport \"{path}\" .")))
                .unwrap_err()
                .contains("normal relative path")
        );
    }
    let outside = tempdir().unwrap();
    let dir = tempdir().unwrap();
    fs::write(outside.path().join("payload"), "[]").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("payload"),
        dir.path().join("scenarios.json"),
    )
    .unwrap();
    fs::write(dir.path().join("domain.ttl"), source()).unwrap();
    assert!(
        load_source(dir.path(), &demo().join("dependencies.lock.json"), cache())
            .unwrap_err()
            .contains("escapes root")
    );
}

#[test]
fn reference_errors_sort_by_subject_then_predicate() {
    let broken = source()
        .replace(
            "d:Account a h:Record ; h:field d:id",
            "d:Account a h:Record ; h:field d:Absent",
        )
        .replace("h:response d:Account", "h:response d:Unknown");
    assert!(
        check(&broken)
            .unwrap_err()
            .starts_with("urn:oddlang:ledger-http:Account urn:oddlang:http:field")
    );
    let broken = source()
        .replace("h:response d:Account", "h:response d:Unknown")
        .replace("h:error he:AccountMissing", "h:error he:Unknown");
    assert!(
        check(&broken)
            .unwrap_err()
            .starts_with("urn:oddlang:ledger-http:LookupAccount urn:oddlang:http:error")
    );
}

#[test]
fn all_guard_errors_need_http_mappings() {
    assert!(
        check(&source().replace(", e:DestinationOverflow .", " ."))
            .unwrap_err()
            .contains("missing mapping for urn:oddlang:ledger:error:DestinationOverflow")
    );
}

#[test]
fn invalid_legacy_pin_precedes_project_import_check() {
    let dir = tempdir().unwrap();
    let lock = dir.path().join("lock.json");
    fs::write(
        &lock,
        r#"{"identity":"urn:invalid","version":"v1","manifestSha256":"unused"}"#,
    )
    .unwrap();
    assert_eq!(
        oddlang::contracts::resolve(&demo().join("project.ttl"), &lock, cache())
            .err()
            .unwrap(),
        "unsupported release identity or invalid version"
    );
}

struct Published {
    dir: tempfile::TempDir,
    release: PathBuf,
    lock: PathBuf,
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Published {
    fn copy() -> Self {
        fn copy_dir(from: &Path, to: &Path) {
            fs::create_dir_all(to).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let dest = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir(&entry.path(), &dest)
                } else {
                    fs::copy(entry.path(), dest).unwrap();
                }
            }
        }
        let dir = tempdir().unwrap();
        let rel = PathBuf::from("http")
            .join(digest(b"urn:oddlang:contract:ledger-http"))
            .join("v1");
        let release = dir.path().join(&rel);
        copy_dir(&cache().join(rel), &release);
        copy_dir(&cache().join("ledger/v1"), &dir.path().join("ledger/v1"));
        let lock = dir.path().join("workspace.json");
        fs::copy(demo().join("workspace.lock.json"), &lock).unwrap();
        Self { dir, release, lock }
    }

    fn modify_manifest(&self, edit: impl FnOnce(&mut serde_json::Value)) {
        let manifest = self.release.join("manifest.json");
        let mut data = read_json(&manifest);
        edit(&mut data);
        let bytes = serde_json::to_vec(&data).unwrap();
        let mut lock = read_json(&self.lock);
        lock["pins"][0]["manifestSha256"] = digest(&bytes).into();
        fs::write(manifest, bytes).unwrap();
        fs::write(&self.lock, lock.to_string()).unwrap();
    }

    fn error(&self) -> String {
        load(&demo().join("project.ttl"), &self.lock, self.dir.path()).unwrap_err()
    }
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn workspace_cannot_override_release_dependency() {
    let f = Published::copy();
    let mut lock = read_json(&f.lock);
    lock["pins"]
        .as_array_mut()
        .unwrap()
        .push(read_json(Path::new("contracts/workspace-v2.lock.json")));
    fs::write(&f.lock, lock.to_string()).unwrap();
    assert!(f.error().contains("conflicting dependency pin"));
}

#[test]
fn manifest_bytes_and_declared_identity_match_pin() {
    let f = Published::copy();
    let path = f.release.join("manifest.json");
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b' ');
    fs::write(&path, bytes).unwrap();
    assert!(f.error().contains("digest mismatch for manifest.json"));
    f.modify_manifest(|manifest| manifest["identity"] = "urn:different".into());
    assert!(f.error().contains("manifest disagrees with pin"));
}

#[test]
fn published_ontology_must_be_reachable() {
    let f = Published::copy();
    fs::write(f.release.join("extra.ttl"), b"").unwrap();
    f.modify_manifest(|manifest| manifest["files"]["extra.ttl"] = digest(b"").into());
    assert!(f.error().contains("unreachable ontology payload extra.ttl"));
}

#[cfg(unix)]
#[test]
fn published_payload_symlink_must_remain_in_release() {
    let f = Published::copy();
    let outside = tempdir().unwrap();
    let path = f.release.join("scenarios.json");
    let target = outside.path().join("scenarios.json");
    fs::rename(&path, &target).unwrap();
    std::os::unix::fs::symlink(target, path).unwrap();
    assert!(f.error().contains("cache path escapes root"));
}
