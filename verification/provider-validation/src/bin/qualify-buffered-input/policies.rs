use super::*;

fn recover(s: &Suite, source: &str, selection: &Value) -> Result<(Value, Value)> {
    let original = "original.json";
    let changed = "reordered.json";
    s.shape(original)?;
    let edit = "    feed: T,\n    octets: zeroize::Zeroizing<[u8; 7]>,";
    assert!(source.contains(edit));
    let lib = s.root.join("src/lib.rs");
    fs::write(
        &lib,
        source.replace(
            edit,
            "    octets: zeroize::Zeroizing<[u8; 11]>,\n    feed: T,",
        ),
    )?;
    s.shape(changed)?;
    fs::remove_file(lib)?;
    write(&s.root.join("selection.json"), selection)?;
    for (input, name, capacity, order) in [
        (
            original,
            "baseline",
            7,
            ["input", "storage", "begin", "end"],
        ),
        (
            changed,
            "mutation",
            11,
            ["storage", "input", "begin", "end"],
        ),
    ] {
        let args = ["recover", "buffered-input", input, "selection.json"];
        s.fresh(&args, name, None)?;
        let repeat = format!("{name}-repeat");
        s.fresh(&args, &repeat, None)?;
        let out = s.root.join(name);
        let artifacts = files(&out)?;
        assert_eq!(
            artifacts.keys().map(String::as_str).collect::<Vec<_>>(),
            ["graph.json", "recovery.lock", "rust.json"]
        );
        assert_eq!(artifacts, files(&s.root.join(repeat))?);
        let graph = read(&out.join("graph.json"))?;
        assert_eq!(graph["capacity"], capacity);
        assert_eq!(graph["fieldOrder"], json!(order));
        let lock = read(&out.join("recovery.lock"))?;
        assert_eq!(lock["observationSha256"], sha(&s.root.join(input))?);
        assert_eq!(
            lock["selectionSha256"],
            sha(&s.root.join("selection.json"))?
        );
    }
    s.existing(
        &["recover", "buffered-input", original, "selection.json"],
        "baseline",
    )?;
    let raw = read(&s.root.join(original))?;
    let mut shadow = raw.clone();
    let entry = shadow["graph"]["declarations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["id"] == selection["declaration"])
        .unwrap();
    let storage = entry["fields"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["name"] == "octets")
        .unwrap();
    storage["type"]["id"] = json!("local::light_meter::Zeroizing::{type}");
    let mut gap = raw;
    gap["graph"]["gaps"].as_array_mut().unwrap().push(json!({"owner":format!("{}::used",selection["declaration"].as_str().unwrap()),"reason":"fixture-incomplete"}));
    for (name, data, error) in [
        ("shadow", shadow, "expected fixed erasing octet storage"),
        ("gap", gap, "unresolved selected buffer shape"),
    ] {
        write(&s.root.join("invalid.json"), &data)?;
        s.fresh(
            &[
                "recover",
                "buffered-input",
                "invalid.json",
                "selection.json",
            ],
            name,
            Some(error),
        )?;
    }
    Ok((
        read(&s.root.join("baseline/graph.json"))?,
        read(&s.root.join("baseline/rust.json"))?,
    ))
}
fn package(s: &Suite, graph: &Value, overlay: &Value) -> Result<Value> {
    write(&s.root.join("graph.json"), graph)?;
    write(&s.root.join("rust.json"), overlay)?;
    s.manifest("graph.json", "rust.json", "meter")
}
fn controls(s: &Suite, graph: &Value, overlay: &Value) -> Result {
    let base = package(s, graph, overlay)?;
    for (key, value, out, diag) in [
        (
            "visibility",
            json!("public"),
            "public",
            "buffered-input must stay private",
        ),
        (
            "reexports",
            json!(["Lookahead"]),
            "reexport",
            "buffered-input must stay private",
        ),
    ] {
        let mut bad = base.clone();
        bad["components"][0][key] = value;
        s.assemble(&bad, out, Some(diag))?;
    }
    for (key, value, out, diag) in [
        ("capacity", json!(0), "zero", "nonempty fixed buffer"),
        ("capacity", json!(-1), "negative", "invalid value"),
        ("capacity", json!(1u64 << 63), "too-big", "layout bound"),
        (
            "fieldOrder",
            json!(["input", "input", "input", "input"]),
            "duplicate-role",
            "each buffered-input field role",
        ),
    ] {
        let mut bad = graph.clone();
        bad[key] = value;
        s.assemble(&package(s, &bad, overlay)?, out, Some(diag))?;
    }
    let mut bad = package(s, graph, overlay)?;
    bad["dependencies"] = json!({});
    s.assemble(
        &bad,
        "missing-provider",
        Some("locked registry provider zeroize"),
    )?;
    let lock = s.root.join("Cargo.lock");
    let original = fs::read_to_string(&lock)?;
    assert!(original.contains("e1308439"));
    fs::write(&lock, original.replace("e1308439", "00000000"))?;
    s.assemble(
        &package(s, graph, overlay)?,
        "altered-provider",
        Some("locked registry source and checksum"),
    )?;
    fs::write(lock, original)?;
    let mut bad = overlay.clone();
    bad["parameter"] = json!("usize");
    s.assemble(
        &package(s, graph, &bad)?,
        "shadow-parameter",
        Some("invalid buffered-input Rust binding"),
    )
}
pub(super) fn verify(s: &Suite, source: &str) -> Result {
    let selection = json!({"id":"urn:test:meter:lookahead","declaration":"local::light_meter::Lookahead::{type}",
        "fields":{"input":"feed","storage":"octets","begin":"used","end":"loaded"},
        "transitions":{"emptyRead":"refill-if-exhausted","refillError":"reset-begin-retain-end","consumedBytes":"erase","releaseStorage":"erase"}});
    let (graph, overlay) = recover(s, source, &selection)?;
    controls(s, &graph, &overlay)?;
    let base = package(s, &graph, &overlay)?;
    for out in ["stable-a", "stable-b"] {
        s.assemble(&base, out, None)?;
    }
    for name in ["package.lock", LEAF] {
        assert_eq!(
            fs::read(s.root.join("stable-a").join(name))?,
            fs::read(s.root.join("stable-b").join(name))?
        );
    }
    let template = fs::read_to_string(s.fixture.join("probe.rs.in"))?;
    for empty in [false, true] {
        for stale in [false, true] {
            for erase in [false, true] {
                let name = format!("policy-{empty}-{stale}-{erase}");
                let mut model = graph.clone();
                model["transitions"]["emptyRead"] = json!(if empty {
                    "return-zero"
                } else {
                    "refill-if-exhausted"
                });
                model["transitions"]["refillError"] = json!(if stale {
                    "reset-begin-retain-end"
                } else {
                    "discard-window"
                });
                model["transitions"]["consumedBytes"] =
                    json!(if erase { "erase" } else { "retain" });
                s.assemble(&package(s, &model, &overlay)?, &name, None)?;
                let product = s.root.join(&name);
                let lock = read(&product.join("package.lock"))?;
                assert_eq!(lock["generatedFiles"][LEAF], sha(&product.join(LEAF))?);
                assert!(!fs::read_to_string(product.join(LEAF))?.contains("pub "));
                let probe = template
                    .replace("EMPTY_CALLS", if empty { "0" } else { "1" })
                    .replace("OLD_END", if stale { "3" } else { "0" })
                    .replace("AFTER_TWO", if erase { "0, 0" } else { "17, 41" })
                    .replace("AFTER_ALL", if erase { "0, 0, 0" } else { "17, 41, 83" });
                s.attach(&name, &probe)?;
                s.probe(&product, None)?;
            }
        }
    }
    Ok(())
}
