use super::*;
pub(super) fn verify(s: &Suite, raw: &Value, binding: &Value, flat: &Value) -> Result {
    let graph = s.fixture.join("graph.json");
    let overlay = s.fixture.join("rust.json");
    let graph_pin = s.digest("digest", &graph, None)?;
    let state = format!("{PREFIX}state");
    let mut bad = flat.clone();
    bad["storage"][&state]["end"] = json!("consumed");
    s.digest(
        "overlay-digest",
        &s.save("duplicate-storage", &bad)?,
        Some("duplicate private journal"),
    )?;
    let mut bad = flat.clone();
    bad["storage"][format!("{PREFIX}unknown")] = bad["storage"][&state].clone();
    let unknown = s.save("unknown-storage", &bad)?;
    s.build(
        &graph,
        &unknown,
        "unknown",
        &graph_pin,
        &s.digest("overlay-digest", &unknown, None)?,
        Some("unknown journal state"),
    )?;
    for (name, defs, error) in [
        (
            "cyclic",
            json!({"a":{"expr":"b"},"b":{"expr":"a"}}),
            "named expression cycle",
        ),
        (
            "missing",
            json!({"unused":{"expr":"missing"}}),
            "unknown named expression",
        ),
        (
            "bad-use",
            json!({"usable":{"arg":"unknown"}}),
            "unknown argument",
        ),
    ] {
        let mut bad = raw.clone();
        bad["expressions"]
            .as_object_mut()
            .unwrap()
            .extend(defs.as_object().unwrap().clone());
        s.digest("digest", &s.save(name, &bad)?, Some(error))?;
    }
    let mut bad = raw.clone();
    op(&mut bad, "view")["cases"][0]["when"] = json!([{"cursor":0}]);
    s.digest(
        "digest",
        &s.save("invalid-type", &bad)?,
        Some("expected Flag, found Cursor"),
    )?;
    let mut bad = raw.clone();
    op(&mut bad, "ack")["access"] = json!("read");
    s.digest(
        "digest",
        &s.save("invalid-write", &bad)?,
        Some("only fallible write"),
    )?;
    let mut bad = raw.clone();
    op(&mut bad, "view")["cases"]
        .as_array_mut()
        .unwrap()
        .pop()
        .unwrap();
    s.digest(
        "digest",
        &s.save("missing-final", &bad)?,
        Some("exactly the final case"),
    )?;
    for (doc, field, key, verb) in [
        (raw, "limits", "max-bytes", "digest"),
        (raw, "expressions", "len", "digest"),
        (
            binding,
            "methods",
            "urn:telemetry-queue:usable",
            "overlay-digest",
        ),
    ] {
        let marker = format!("\"{field}\":{{");
        let value = doc[field].get(key).unwrap();
        let encoded = serde_json::to_string(doc)?;
        assert_eq!(encoded.matches(&marker).count(), 1);
        let duplicate = encoded.replacen(
            &marker,
            &format!("{marker}{}:{value},", serde_json::to_string(key)?),
            1,
        );
        let file = s.root.join(format!("duplicate-{field}.json"));
        fs::write(&file, duplicate)?;
        s.digest(verb, &file, Some("duplicate binding key"))?;
    }
    let mut incomplete = binding.clone();
    incomplete["methods"]
        .as_object_mut()
        .unwrap()
        .remove(&format!("{PREFIX}view"))
        .unwrap();
    let incomplete = s.save("incomplete", &incomplete)?;
    s.build(
        &graph,
        &incomplete,
        "bad-overlay",
        &graph_pin,
        &s.digest("overlay-digest", &incomplete, None)?,
        Some("binding must cover"),
    )?;
    s.build(
        &graph,
        &overlay,
        "bad-digest",
        &"0".repeat(64),
        &s.digest("overlay-digest", &overlay, None)?,
        Some("graph digest mismatch"),
    )?;
    let mut shadow = binding.clone();
    shadow["types"][&state] = json!("Result");
    let shadow = s.save("shadowed-rust", &shadow)?;
    s.build(
        &graph,
        &shadow,
        "shadowed",
        &graph_pin,
        &s.digest("overlay-digest", &shadow, None)?,
        Some("reserved journal Rust type"),
    )?;
    Ok(())
}
