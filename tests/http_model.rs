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
    let dir = tempdir().unwrap();
    let hash = fs::read_dir(cache().join("http"))
        .unwrap()
        .map(|p| p.unwrap())
        .find(|entry| {
            let file = entry.path().join("v1/manifest.json");
            fs::read_to_string(file).is_ok_and(|s| s.contains("urn:oddlang:contract:ledger-http"))
        })
        .unwrap()
        .file_name();
    let rel = PathBuf::from("http").join(hash).join("v1");
    let root = dir.path().join(&rel);
    fs::create_dir_all(&root).unwrap();
    for f in ["domain.ttl", "scenarios.json", "manifest.json"] {
        fs::copy(cache().join(&rel).join(f), root.join(f)).unwrap();
    }
    let mut payload = fs::read(root.join("domain.ttl")).unwrap();
    payload[0] ^= 1;
    fs::write(root.join("domain.ttl"), payload).unwrap();
    assert!(
        load(
            &demo().join("project.ttl"),
            &demo().join("workspace.lock.json"),
            dir.path()
        )
        .unwrap_err()
        .contains("digest mismatch for domain.ttl")
    );
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
