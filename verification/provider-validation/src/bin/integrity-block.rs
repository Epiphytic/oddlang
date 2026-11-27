//! Compare integrity-block projection invariants and run the independent codec observer.
use provider_validation::{Result, cargo, cli, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../integrity-block/fixture");
    let consumer = root.join("consumer");
    fs::create_dir_all(consumer.join("src"))?;
    for file in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
        fs::copy(fixture.join(file), consumer.join(file))?;
    }
    let graph = read(&fixture.join("graph.json"))?;
    let binding = read(&fixture.join("rust.json"))?;
    let args = [
        "build",
        "integrity-block",
        "rust",
        "graph.json",
        "rust.json",
    ];
    let project = |g: &Value, b: &Value, name: &str, reason: Option<&str>| -> Result {
        write(&root.join("graph.json"), g)?;
        write(&root.join("rust.json"), b)?;
        invoke(&cli, root, &args, name, reason)
    };
    project(&graph, &binding, "baseline", None)?;
    // Serialize the keys in reverse order without changing any field value.
    let entries = graph
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(key, value)| Ok(format!("{}:{value}", serde_json::to_string(key)?)))
        .collect::<Result<Vec<_>>>()?;
    fs::write(
        root.join("graph.json"),
        format!("{{{}}}", entries.join(",")),
    )?;
    invoke(&cli, root, &args, "repeat", None)?;
    let same = |file: &str, output: &str| -> Result {
        assert_eq!(
            fs::read(root.join("baseline").join(file))?,
            fs::read(root.join(output).join(file))?,
            "{output}/{file}"
        );
        Ok(())
    };
    for file in ["codec.rs", "obligations.json", "projection.lock"] {
        same(file, "repeat")?;
    }
    let lock = read(&root.join("baseline/projection.lock"))?;
    for (file, key) in [
        ("codec.rs", "artifactSha256"),
        ("obligations.json", "obligationsSha256"),
    ] {
        assert_eq!(lock[key], sha(&root.join("baseline").join(file))?);
    }
    let obligations = read(&root.join("baseline/obligations.json"))?;
    let rows = obligations["obligations"].as_array().unwrap();
    let mut ids = BTreeSet::new();
    for row in rows {
        assert!(ids.insert(row["declaration"].as_str().ok_or("missing declaration")?));
        assert_eq!(row["qualification"], "independent-witness-required");
    }
    let mut hintless = graph.clone();
    hintless
        .as_object_mut()
        .unwrap()
        .remove("providerHints")
        .unwrap();
    project(&hintless, &binding, "hintless", None)?;
    same("codec.rs", "hintless")?;
    same("obligations.json", "hintless")?;
    let mut identity = graph.clone();
    identity["representations"]["compressed"] = Value::Null;
    let mut native = binding.clone();
    native["variants"]
        .as_object_mut()
        .unwrap()
        .remove("compressed")
        .unwrap();
    native
        .as_object_mut()
        .unwrap()
        .remove("payloadHelper")
        .unwrap();
    project(&identity, &native, "identity", None)?;
    assert!(!fs::read_to_string(root.join("identity/codec.rs"))?.contains("::flate2"));
    let mut messages = graph.clone();
    messages["errorMessages"] = Value::Object(
        graph["errors"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(role, id)| {
                (
                    id.as_str().unwrap().to_owned(),
                    json!(format!("wire {role}")),
                )
            })
            .collect(),
    );
    project(&messages, &binding, "messages", None)?;
    let mut bad = binding.clone();
    bad["blockType"] = json!("u64");
    project(&graph, &bad, "invalid", Some("must not shadow a primitive"))?;
    let mut changed = graph.clone();
    changed["fields"][0]["wireName"] = json!("location");
    changed["bounds"]["offsetMaximum"] = json!(31);
    changed["validateOrder"] = json!(["offset", "raw-size", "encoded-size", "representation-size"]);
    project(&changed, &binding, "mutant", None)?;
    let mut narrower = graph.clone();
    narrower["bounds"]["encodedMaximum"] = json!(64);
    project(&narrower, &binding, "narrower", None)?;
    for (name, pointer, value, reason) in [
        (
            "duplicate",
            "/fields/1/wireName",
            json!("position"),
            "distinct wire names",
        ),
        (
            "dictionary",
            "/representations/compressed/dictionary",
            json!("allowed"),
            "unsupported compression policy",
        ),
    ] {
        let mut bad = graph.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        project(&bad, &binding, name, Some(reason))?;
    }
    let mut bad = graph;
    bad["decodeOrder"].as_array_mut().unwrap().reverse();
    project(&bad, &binding, "order", Some("decode precedence"))?;
    for (projection, module) in [
        ("baseline", "codec"),
        ("identity", "stored"),
        ("messages", "all_messages"),
        ("mutant", "mutant"),
        ("narrower", "narrower"),
    ] {
        fs::copy(
            root.join(projection).join("codec.rs"),
            consumer.join("src").join(format!("{module}.rs")),
        )?;
    }
    cargo(&consumer, &["run", "--locked", "--offline", "--quiet"])?;
    println!("integrity-block qualification passed");
    Ok(())
}
