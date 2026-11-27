use super::*;
use std::collections::BTreeSet;

const SENSOR: &str = "urn:garden:sensor";
const EVENT: &str = "urn:garden:event";

fn evidence(s: &Suite, selection: &Value, source: &str) -> Result {
    let observations = read(&s.root.join("observed/observation.json"))?;
    let gaps = rows(&observations["gaps"]);
    assert!(gaps.iter().all(|r| r["kind"] == "behavior"));
    let reasons: BTreeSet<_> = gaps.iter().map(|r| r["reason"].as_str().unwrap()).collect();
    assert_eq!(
        reasons,
        BTreeSet::from([
            "conditional guards require independent evidence",
            "selected adapter policy requires independent witness: chunks_text",
            "selected adapter policy requires independent witness: signature_text"
        ])
    );
    fs::write(s.root.join("tampered.rs"), format!("{source}\n"))?;
    invoke(
        &s.cli,
        &s.root,
        &["recover", "wire-schema", "tampered.rs", "selection.json"],
        "bad-source",
        Some("source digest"),
    )?;
    for case in rows(&s.json("lexical-controls.json")?) {
        let name = case["name"].as_str().unwrap();
        let reason = case["reason"].as_str().unwrap();
        let edited = if let Some(old) = case["replace"].as_str() {
            assert!(source.contains(old));
            source.replace(old, case["text"].as_str().unwrap())
        } else {
            format!(
                "{}{source}{}",
                case["prepend"].as_str().unwrap_or(""),
                case["append"].as_str().unwrap_or("")
            )
        };
        let file = s.root.join(format!("{name}.rs"));
        fs::write(&file, edited)?;
        let model = recover(s, &file, selection, name)?;
        let observed = read(&s.root.join(name).join("observation.json"))?;
        assert!(rows(&observed["gaps"]).iter().any(|g|g["kind"]=="structure" && g["reason"].as_str().unwrap().contains(reason)),"{name}");
        s.project(
            &model,
            &format!("{name}-unsafe"),
            Some("unresolved structural evidence"),
        )?;
    }
    Ok(())
}

fn valves(s: &Suite, consumer: &Path) -> Result {
    let source = s.case("valve.rs");
    let selection = json!({"id":"urn:garden:valves","types":{"Valve":"urn:garden:valve"}});
    let model = recover(s, &source, &selection, "valve")?;
    recover(s, &source, &selection, "valve-repeat")?;
    s.compare("valve", "valve-repeat", OBSERVED)?;
    assert_eq!(model["graph"]["types"][0]["kind"], "unit-enum");
    let empty = s.root.join("empty.rs");
    fs::write(
        &empty,
        "use serde::{Serialize,Deserialize}; #[derive(Serialize,Deserialize)] pub enum Valve {}",
    )?;
    recover(s, &empty, &selection, "empty")?;
    let observed = read(&s.root.join("empty/observation.json"))?;
    assert!(rows(&observed["gaps"]).iter().any(|g| {
        g["reason"]
            .as_str()
            .unwrap()
            .contains("not explicitly adjacent-tagged")
    }));
    for (name, error) in [
        ("empty", "distinct, nonempty"),
        ("duplicate", "distinct, nonempty"),
        ("payload", "unit payloads"),
    ] {
        let mut bad = model.clone();
        let variants = bad["graph"]["types"][0]["variants"].as_array_mut().unwrap();
        match name {
            "empty" => variants.clear(),
            "duplicate" => variants[1]["name"] = variants[0]["name"].clone(),
            _ => variants[0]["payload"] = json!({"kind":"record","fields":[]}),
        }
        s.project(&bad, &format!("valve-{name}-bad"), Some(error))?;
    }
    s.project(&model, "valve-projection", None)?;
    fs::copy(
        s.root.join("valve-projection/wire.rs"),
        consumer.join("src/valve.rs"),
    )?;
    fs::copy(source, consumer.join("src/source_valve.rs"))?;
    Ok(())
}

fn presence(s: &Suite, selection: &Value, source: &str, model: &Value, consumer: &Path) -> Result {
    let mut strict = model.clone();
    let sensor = find_mut(&mut strict["graph"]["types"], "id", &json!(SENSOR));
    let field = find_mut(&mut sensor["fields"], "name", &json!("meter"));
    let id = field["id"].clone();
    field["requirePresence"] = json!(true);
    s.project(&strict, "strict", None)?;
    fs::copy(
        s.root.join("strict/wire.rs"),
        consumer.join("src/strict.rs"),
    )?;
    let catalog = read(&s.root.join("strict/obligations.json"))?;
    assert_eq!(
        find(&catalog["obligations"], "declaration", &id)["expected"]["parsePresence"],
        "required-value-or-null"
    );
    find_mut(&mut strict["graph"]["types"], "id", &json!(SENSOR))["fields"][0]["requirePresence"] =
        json!(true);
    s.project(
        &strict,
        "bad-presence",
        Some("requirePresence applies to a nullable field"),
    )?;
    for helper in [
        "::serde::Deserialize::deserialize",
        "read_nullable",
        "unreviewed",
    ] {
        let mut policy = selection.clone();
        if helper == "read_nullable" {
            policy["adapters"][helper] = json!({"kind":"required-nullable"});
        }
        let authored = s.root.join("presence-source.rs");
        fs::write(
            &authored,
            source.replace(
                "pub meter:",
                &format!("#[serde(deserialize_with = \"{helper}\")]\n    pub meter:"),
            ),
        )?;
        let data = recover(
            s,
            &authored,
            &policy,
            &format!("presence-{}", helper.replace(':', "_")),
        )?;
        let sensor = find(&data["graph"]["types"], "id", &json!(SENSOR));
        let field = find(&sensor["fields"], "id", &id);
        if helper == "unreviewed" {
            assert!(rows(&data["graph"]["unresolved"]).iter().any(|r| {
                r.as_str()
                    .unwrap()
                    .contains("unverified Serde deserializer")
            }));
        } else {
            assert_eq!(field["requirePresence"], true);
        }
    }
    Ok(())
}

pub(super) fn verify(s: &Suite) -> Result {
    let fixture = s.fixtures.join("fixture");
    let path = fixture.join("input.rs");
    let source = fs::read_to_string(&path)?;
    let selection = s.json("garden-selection.json")?;
    let mut model = recover(s, &path, &selection, "observed")?;
    recover(s, &path, &selection, "repeat")?;
    s.compare("observed", "repeat", OBSERVED)?;
    evidence(s, &selection, &source)?;
    let event = find(&model["graph"]["types"], "id", &json!(EVENT)).clone();
    let adjust = find(&event["variants"], "name", &json!("adjust_rate"));
    let field = |name: &str| find(&adjust["payload"]["fields"], "name", &json!(name))["id"].clone();
    model["graph"]["guards"] = json!([
        {"id":"urn:garden:rule:flow","owner":EVENT,"variant":adjust["id"],"when":{"kind":"bool","field":field("enabled"),"value":true},
        "require":{"kind":"unsigned-range","field":field("flow"),"minimum":1,"maximum":20},"error":"urn:garden:flow"},
        {"id":"urn:garden:rule:tag","owner":EVENT,"variant":adjust["id"],"require":{"kind":"unique","field":field("tags")},"error":"urn:garden:tag"}]);
    s.project(&model, "native", None)?;
    let mut reversed = model.clone();
    reversed["graph"]["types"].as_array_mut().unwrap().reverse();
    s.project(&reversed, "reordered", None)?;
    s.compare("native", "reordered", PROJECTED)?;
    assert_eq!(
        read(&s.root.join("native/projection.lock"))?["obligationsSha256"],
        sha(&s.root.join("native/obligations.json"))?
    );
    let mut bad = model.clone();
    bad["binding"]["derives"][SENSOR] = json!(["Clone", "Copy"]);
    s.project(
        &bad,
        "bad-derive",
        Some("referenced field does not support derive Eq"),
    )?;
    let mut bad = model.clone();
    bad["binding"]["derives"][EVENT]
        .as_array_mut()
        .unwrap()
        .push(json!("Copy"));
    s.project(
        &bad,
        "bad-copy",
        Some("referenced field does not support derive Copy"),
    )?;
    let consumer = s.root.join("consumer");
    fs::create_dir_all(consumer.join("src"))?;
    for name in ["Cargo.toml", "Cargo.lock", "input.rs", "src/main.rs"] {
        fs::copy(fixture.join(name), consumer.join(name))?;
    }
    fs::copy(
        s.root.join("native/wire.rs"),
        consumer.join("src/projected.rs"),
    )?;
    valves(s, &consumer)?;
    presence(s, &selection, &source, &model, &consumer)?;
    let mut single = model.clone();
    find_mut(&mut single["graph"]["types"], "id", &json!(EVENT))["variants"] = json!([adjust]);
    for variant in rows(&event["variants"]) {
        if variant["id"] != adjust["id"] {
            single["binding"]["names"]
                .as_object_mut()
                .unwrap()
                .remove(variant["id"].as_str().unwrap())
                .unwrap();
        }
    }
    s.project(&single, "single", None)?;
    fs::copy(
        s.root.join("single/wire.rs"),
        consumer.join("src/single.rs"),
    )?;
    let run = &["run", "--quiet", "--locked", "--offline"];
    command(
        Command::new("cargo")
            .env("CARGO_TARGET_DIR", consumer.join("target"))
            .env("CARGO_ENCODED_RUSTFLAGS", "-Dwarnings"),
        &consumer,
        run,
        None,
    )?;
    model["graph"]["guards"].as_array_mut().unwrap().reverse();
    s.project(&model, "mutant", None)?;
    fs::copy(
        s.root.join("mutant/wire.rs"),
        consumer.join("src/projected.rs"),
    )?;
    command(
        Command::new("cargo").env("CARGO_TARGET_DIR", consumer.join("target")),
        &consumer,
        run,
        Some("Err(\"urn:garden:flow\")"),
    )?;
    model["graph"]["guards"][0]["require"]["field"] = field("flow");
    s.project(&model, "bad-guard", Some("predicate field type"))
}
