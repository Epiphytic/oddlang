use oddlang::wasm::{
    bundle::{publish, verify_bundle},
    model::World,
    wit::import_dir,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn fixture(name: &str) -> PathBuf {
    Path::new("contracts/wasm").join(name)
}
fn clean_publish(name: &str, out: &Path) {
    publish(
        &fixture(name),
        &fixture("empty.lock.json"),
        Path::new("contracts/cache"),
        out,
    )
    .unwrap();
}
fn load(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write(path: impl AsRef<Path>, v: &Value) {
    fs::write(path, serde_json::to_vec_pretty(v).unwrap()).unwrap()
}
fn copy_source(name: &str, out: &Path) {
    fs::create_dir_all(out).unwrap();
    for file in ["model.ttl", "corpus.json", "dependencies.json"] {
        fs::copy(fixture(name).join(file), out.join(file)).unwrap();
    }
}

#[test]
fn two_clean_releases_and_roundtrip_all_selected_worlds() {
    let temp = tempdir().unwrap();
    for (name, world_name) in [
        ("ledger", "ledger"),
        ("ledger-test", "ledger-test"),
        ("echo", "echo-world"),
    ] {
        let first = temp.path().join(name);
        clean_publish(name, &first);
        let second = temp.path().join(format!("{name}-again"));
        clean_publish(name, &second);
        let manifest = load(first.join("manifest.json"));
        for file in manifest["files"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .chain(["manifest.json", "workspace.lock.json"])
        {
            assert_eq!(
                fs::read(first.join(file)).unwrap(),
                fs::read(second.join(file)).unwrap(),
                "{file}"
            );
        }
        let release = verify_bundle(&first, &first.join("workspace.lock.json")).unwrap();
        assert_eq!(release.world.name(), world_name);
        let ids: BTreeMap<String, String> =
            serde_json::from_value(load(first.join("identities.json"))).unwrap();
        let recovered = import_dir(&first.join("wit"), world_name, &ids, None).unwrap();
        assert_eq!(ids, recovered.identities().unwrap());
        assert_eq!(
            release.world.behavior_roles().unwrap(),
            recovered.behavior_roles().unwrap()
        );
        if name.starts_with("ledger") {
            for func in ["fixture-load", "complete-snapshot"] {
                assert_eq!(
                    ids.contains_key(&format!("world/export/{func}")),
                    name == "ledger-test"
                );
            }
            assert!(ids.contains_key("world/import/initial-state"));
            assert!(ids.contains_key("world/export/lookup"));
            assert!(ids.contains_key("world/export/transfer"));
        }
        let cases = load(first.join("cases.json"));
        let obligations = load(first.join("obligations.json"));
        for row in obligations
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == "behavior")
        {
            assert!(
                row["caseIds"].as_array().unwrap().iter().all(|id| cases
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["caseId"] == *id))
            );
        }
    }
}

#[test]
fn equivalent_graph_order_and_reordered_variant_abi() {
    let original = fs::read_to_string(fixture("echo/model.ttl")).unwrap();
    let (prefix, body): (Vec<_>, Vec<_>) = original
        .lines()
        .filter(|l| !l.is_empty())
        .partition(|l| l.starts_with("@prefix"));
    let reversed = prefix
        .into_iter()
        .chain(body.into_iter().rev())
        .collect::<Vec<_>>()
        .join("\n");
    let model = World::from_turtle(original.as_bytes()).unwrap();
    let reordered = World::from_turtle(reversed.as_bytes()).unwrap();
    assert_eq!(model.render().unwrap(), reordered.render().unwrap());
    assert_eq!(model.identities().unwrap(), reordered.identities().unwrap());
    let swapped = original
        .replace(
            "w:name \"empty\" ; w:ordinal 0",
            "w:name \"empty\" ; w:ordinal 1",
        )
        .replace(
            "w:name \"number\" ; w:ordinal 1",
            "w:name \"number\" ; w:ordinal 0",
        );
    let variant = World::from_turtle(swapped.as_bytes()).unwrap();
    assert_ne!(model.render().unwrap(), variant.render().unwrap());
    let invalid = original.replace(
        "w:name \"empty\" ; w:ordinal 0",
        "w:name \"empty\" ; w:ordinal 1",
    );
    assert!(World::from_turtle(invalid.as_bytes()).is_err());
}

#[test]
fn uncovered_invented_or_false_associations_do_not_publish() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    copy_source("ledger", &source);
    let corpus = load(source.join("corpus.json"));
    for edit in ["missing", "false", "invented", "constraint", "duplicate"] {
        let mut bad = corpus.clone();
        match edit {
            "missing" => {
                bad[0]["covers"].as_array_mut().unwrap().remove(1);
            }
            "false" => {
                bad[0]["imports"] = json!([]);
            }
            "constraint" => {
                bad[1]["constraints"] = json!([]);
            }
            "invented" => {
                bad[0]["calls"][0]["function"] = json!("typo");
            }
            _ => {
                let c = bad[0].clone();
                bad.as_array_mut().unwrap().push(c);
            }
        }
        write(source.join("corpus.json"), &bad);
        let out = temp.path().join(edit);
        assert!(
            publish(&source, &fixture("empty.lock.json"), temp.path(), &out).is_err(),
            "{edit}"
        );
        assert!(!out.exists());
    }
}

#[test]
fn reject_stale_manifest_payload_and_malformed_or_unmapped_wit() {
    let temp = tempdir().unwrap();
    let out = temp.path().join("release");
    clean_publish("echo", &out);
    let lock = out.join("workspace.lock.json");
    let mut stale = load(&lock);
    stale["pins"][0]["manifestSha256"] = json!("0".repeat(64));
    write(temp.path().join("stale.lock"), &stale);
    assert!(verify_bundle(&out, &temp.path().join("stale.lock")).is_err());
    assert!(import_dir(&out.join("wit"), "echo-world", &BTreeMap::new(), None).is_err());
    let allocated = import_dir(
        &out.join("wit"),
        "echo-world",
        &BTreeMap::new(),
        Some("urn:allocated:"),
    )
    .unwrap();
    assert!(
        allocated
            .identities()
            .unwrap()
            .values()
            .all(|i| i.starts_with("urn:allocated:"))
    );
    fs::write(out.join("wit/world.wit"), "garbage").unwrap();
    assert!(verify_bundle(&out, &lock).is_err());
    assert!(
        import_dir(
            &out.join("wit"),
            "echo-world",
            &BTreeMap::new(),
            Some("urn:allocated:")
        )
        .is_err()
    );
}

fn cache_dir(root: &Path, identity: &str) -> PathBuf {
    let hash: String = Sha256::digest(identity.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    root.join(format!("wasm/{hash}/1.0.0"))
}
fn foreign_source(root: &Path, name: &str, dependency: &str, path: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("model.ttl"),
        format!(
            r#"
@prefix w: <urn:oddlang:wasm:> .
@prefix e: <urn:sample:{name}:> .
e:pkg a w:Package ; w:name "sample:{name}@1.0.0" .
e:world a w:World ; w:name "consumer" ; w:use e:message ; w:export e:read .
e:message a w:ForeignType ; w:name "message" ; w:from "{path}" .
e:read a w:Function ; w:name "read" ; w:result "message" .
"#
        ),
    )
    .unwrap();
    write(root.join("dependencies.json"), &json!([dependency]));
    write(
        root.join("corpus.json"),
        &json!([{"caseId":"read","covers":[{"declaration":format!("urn:sample:{name}:read"),"role":"wasm.exporter"}],"imports":[],"calls":[{"function":"read","arguments":[],"expected":{"text":"","items":{"none":true},"choice":{"empty":null}}}]}]),
    );
}

#[test]
fn external_types_require_digest_pinned_transitive_packages() {
    let temp = tempdir().unwrap();
    let cache = temp.path().join("cache");
    let echo_id = "urn:oddlang:echo:package";
    let echo = cache_dir(&cache, echo_id);
    clean_publish("echo", &echo);
    let middle_id = "urn:sample:middle:pkg";
    let middle = cache_dir(&cache, middle_id);
    let source = temp.path().join("middle-src");
    foreign_source(&source, "middle", echo_id, "sample:echo/shapes@1.0.0");
    assert!(publish(&source, &fixture("empty.lock.json"), &cache, &middle).is_err());
    publish(&source, &echo.join("workspace.lock.json"), &cache, &middle).unwrap();
    let lock = middle.join("workspace.lock.json");
    verify_bundle(&middle, &lock).unwrap();

    let mut missing = load(&lock);
    missing["pins"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["identity"] != echo_id);
    let bad = temp.path().join("bad.lock");
    write(&bad, &missing);
    assert!(verify_bundle(&middle, &bad).is_err());

    let consumer = temp.path().join("consumer-src");
    copy_source("ledger", &consumer);
    write(consumer.join("dependencies.json"), &json!([middle_id]));
    let destination = temp.path().join("consumer-release");
    let mut conflicting = load(&lock);
    conflicting["pins"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["identity"] == echo_id)
        .unwrap()["manifestSha256"] = json!("a".repeat(64));
    write(&bad, &conflicting);
    assert!(
        publish(&consumer, &bad, &cache, &destination)
            .unwrap_err()
            .contains("conflicting")
    );
    publish(&consumer, &lock, &cache, &destination).unwrap();
    verify_bundle(&destination, &destination.join("workspace.lock.json")).unwrap();
    fs::write(echo.join("wit/world.wit"), "corrupt").unwrap();
    assert!(publish(&consumer, &lock, &cache, &temp.path().join("invalid")).is_err());
}
