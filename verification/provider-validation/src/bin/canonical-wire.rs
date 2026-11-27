//! Qualify the canonical JSON wire contract against the handwritten radio caller.
use provider_validation::{Result, cli, command, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[derive(Clone)]
struct Radio {
    shape: Value,
    binding: Value,
    protocol: Value,
    target: Value,
}
impl Radio {
    fn load(fixture: &Path) -> Result<Self> {
        Ok(Self {
            shape: read(&fixture.join("shape.json"))?,
            binding: read(&fixture.join("binding.json"))?,
            protocol: read(&fixture.join("protocol.json"))?,
            target: read(&fixture.join("target.json"))?,
        })
    }
    fn build(&self, cli: &Path, root: &Path, name: &str, error: Option<&str>) -> Result {
        for (path, doc) in [
            ("shape.json", &self.shape),
            ("binding.json", &self.binding),
            ("protocol.json", &self.protocol),
            ("target.json", &self.target),
        ] {
            write(&root.join(path), doc)?;
        }
        invoke(
            cli,
            root,
            &[
                "build",
                "wire-protocol",
                "rust",
                "shape.json",
                "binding.json",
                "protocol.json",
                "target.json",
            ],
            name,
            error,
        )
    }
}
fn rejections(cli: &Path, root: &Path, base: &Radio) -> Result {
    let deny = |case: &Radio, why: &str| case.build(cli, root, "denied", Some(why));
    let mut bad = base.clone();
    bad.protocol["endpoints"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .pop();
    deny(&bad, "endpoint must map")?;
    let mut bad = base.clone();
    let tx = bad.protocol["endpoints"][0].as_object_mut().unwrap();
    let timed = tx.remove("timed").unwrap();
    tx.insert("steps".into(), timed);
    bad.target["endpoints"]["urn:radio:tx"]
        .as_object_mut()
        .unwrap()
        .remove("timed");
    deny(&bad, "unknown guard scope clock")?;
    let mut bad = base.clone();
    bad.protocol["endpoints"][0]["timed"][0]["id"] = json!("urn:radio:gain-check");
    deny(&bad, "duplicate wire declaration")?;
    let mut bad = base.clone();
    bad.protocol["digests"][0]["id"] = json!("urn:radio:tx");
    deny(&bad, "duplicate wire declaration")?;
    let mut bad = base.clone();
    bad.shape["types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"record","id":"urn:radio:outer","fields":[{
        "id":"urn:radio:outer:ref","name":"ref","type":{"kind":"ref","id":"urn:radio:message"}}]}));
    bad.binding["names"]["urn:radio:outer"] = json!("Outer");
    bad.binding["names"]["urn:radio:outer:ref"] = json!("frame");
    deny(&bad, "public structural type refers to the internal frame")?;
    let mut bad = base.clone();
    bad.shape["guards"].as_array_mut().unwrap().push(json!({"id":"urn:radio:plain-envelope-check","owner":"urn:radio:envelope",
        "require":{"kind":"unsigned-range","field":"urn:radio:envelope:sequence","minimum":1,"maximum":20},"error":"urn:radio:range"}));
    deny(&bad, "internal frame cannot own")?;
    let mut bad = base.clone();
    bad.protocol["layout"] = json!("unsorted");
    deny(&bad, "unknown variant")?;
    bad.protocol["layout"] = json!("flat-sorted");
    let message = bad.shape["types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["id"] == "urn:radio:message")
        .unwrap();
    message["tag"] = json!("sequence");
    deny(&bad, "flat wire field collision")
}
fn variants(cli: &Path, root: &Path, base: &Radio) -> Result {
    let mut singleton = base.clone();
    singleton.shape["types"][1]["variants"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    for id in ["urn:radio:scan", "urn:radio:read"] {
        singleton.binding["names"]
            .as_object_mut()
            .unwrap()
            .remove(id);
    }
    singleton.protocol["endpoints"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let only = &singleton.protocol["endpoints"][0];
    singleton.target["endpoints"]
        .as_object_mut()
        .unwrap()
        .retain(|id, _| only["id"] == *id);
    singleton.target["fields"]
        .as_object_mut()
        .unwrap()
        .retain(|id, _| {
            only["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["id"] == *id)
        });
    singleton.build(cli, root, "singleton", None)?;
    let mut shape_only = base.clone();
    shape_only.protocol["endpoints"][2]["steps"] = json!([]);
    shape_only.protocol["digests"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["steps"] = json!([]);
    shape_only.build(cli, root, "shape-only", None)?;
    let mut flat = base.clone();
    flat.protocol["layout"] = json!("flat-sorted");
    flat.build(cli, root, "flat", None)?;
    let obligations = read(&root.join("flat/obligations.json"))?;
    let frames: Vec<_> = obligations["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["role"] == "canonical-frame")
        .collect();
    assert_eq!(
        frames.len(),
        base.protocol["endpoints"].as_array().unwrap().len()
    );
    assert!(frames.iter().all(|r| r["layout"] == "flat-sorted"));
    flat.protocol["maximumWireBytes"] = json!(
        br#"{"dir":"send","payload":{"gain":13},"protocol":"radio","sequence":5,"version":7}"#
            .len()
    );
    flat.build(cli, root, "flat-limit", None)
}
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let family = Path::new(env!("CARGO_MANIFEST_DIR")).join("../canonical-wire");
    let base = Radio::load(&family.join("radio"))?;
    base.build(&cli, root, "output", None)?;
    base.build(&cli, root, "repeat", None)?;
    for f in ["wire.rs", "obligations.json", "projection.lock"] {
        assert_eq!(
            fs::read(root.join("output").join(f))?,
            fs::read(root.join("repeat").join(f))?,
            "{f}"
        );
    }
    let lock = read(&root.join("output/projection.lock"))?;
    for (file, key) in [
        ("wire.rs", "wireSha256"),
        ("obligations.json", "obligationsSha256"),
    ] {
        assert_eq!(lock[key], sha(&root.join("output").join(file))?);
    }
    rejections(&cli, root, &base)?;
    variants(&cli, root, &base)?;
    let consumer = root.join("consumer");
    fs::create_dir_all(consumer.join("src"))?;
    for file in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
        fs::copy(family.join("fixture").join(file), consumer.join(file))?;
    }
    for (variant, name) in [
        ("output", "generated"),
        ("singleton", "singleton"),
        ("shape-only", "shape_only"),
        ("flat", "flat"),
        ("flat-limit", "flat_limit"),
    ] {
        fs::copy(
            root.join(variant).join("wire.rs"),
            consumer.join("src").join(format!("{name}.rs")),
        )?;
    }
    command(
        Command::new("cargo").env("CARGO_ENCODED_RUSTFLAGS", "-Dwarnings"),
        &consumer,
        &["run", "--locked", "--offline", "--quiet"],
        None,
    )?;
    println!("canonical source-free native wire operations passed");
    Ok(())
}
