use super::tests::{checked_output, crate_files, fixtures, host};
use super::*;
use std::os::unix::fs::PermissionsExt;

fn indexed(rows: &Value) -> Result<BTreeMap<String, Value>> {
    array(rows)?
        .iter()
        .map(|row| Ok((string(&row["name"])?.to_owned(), row.clone())))
        .collect()
}
pub fn verify(cli: &Path, root: &Path) -> Result {
    let source = fs::read_to_string(fixtures().join("shapes.rs"))?;
    crate_files(
        root,
        "[package]\nname='observatory'\nversion='0.1.0'\nedition='2024'\n[features]\nexpanded=[]\n",
        &source,
    )?;
    let manifest = root.join("Cargo.toml");
    let path = root.join("profile.toml");
    let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rust-recovery/run.sh")
        .canonicalize()?;
    let base = format!(
        "package=\"observatory\"\ntargetKind=\"lib\"\ntargetName=\"observatory\"\ntargetTriple={}\ndefaultFeatures=false\ncompilerCommit=\"{PIN}\"\ncargoLockSha256={}\n",
        json!(host()?),
        json!(sha(&root.join("Cargo.lock"))?)
    );
    let run = |name: &str, error: Option<&str>| {
        checked_output(
            cmd(cli)
                .current_dir(root)
                .args(["recover", "rust-shapes"])
                .arg(&manifest)
                .arg("--profile")
                .arg(&path)
                .arg("--out")
                .arg(root.join(name)),
            error,
        )
    };
    let extract = |source: &str,
                   name: &str,
                   expanded: bool|
     -> Result<(Vec<u8>, Value, BTreeMap<String, Value>)> {
        fs::write(
            &path,
            format!(
                "{base}adapter={}\nadapterInputs=[\"Cargo.toml\",\"Cargo.lock\",\"driver.rs\",\"shapes.rs\",\"shape_model.rs\",\"capture.rs\"]\nfeatures={}\n",
                json!(adapter),
                if expanded { "[\"expanded\"]" } else { "[]" }
            ),
        )?;
        fs::write(root.join("src/lib.rs"), source)?;
        run(name, None)?;
        let dir = root.join(name);
        let lock = read(&dir.join("shapes.lock"))?;
        let graph = dir.join("shapes.json");
        assert_eq!(lock["shapesSha256"], sha(&graph)?);
        assert_eq!(lock["qualification"], "candidate");
        let bytes = fs::read(graph)?;
        let g: Value = serde_json::from_slice(&bytes)?;
        let rows = indexed(&g["declarations"])?;
        Ok((bytes, g, rows))
    };
    let (bytes, g, rows) = extract(&source, "original", false)?;
    assert_eq!(
        rows.keys().collect::<Vec<_>>(),
        ["Bits", "Choice", "Frame", "Marker", "Packet"]
    );
    let frame = &rows["Frame"];
    let fields = indexed(&frame["fields"])?;
    assert_eq!(
        array(&frame["fields"])?
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["word", "shared", "map", "read", "write", "mixed", "array"]
    );
    assert_eq!(
        fields["word"]["type"],
        json!({"kind":"unsigned-integer","bits":16})
    );
    assert_eq!(fields["shared"]["type"]["ownership"], "shared");
    let args = array(&fields["map"]["type"]["arguments"])?;
    assert_eq!(args.len(), 1);
    assert_eq!(args[0]["name"], "T");
    assert_eq!(args[0]["value"]["id"], frame["parameters"][1]["id"]);
    assert_eq!(fields["read"]["type"]["ownership"], "shared-borrow");
    assert_eq!(fields["write"]["type"]["ownership"], "exclusive-borrow");
    assert_eq!(fields["mixed"]["type"]["elements"][1]["position"], 1);
    assert_eq!(
        fields["array"]["type"],
        json!({"kind":"array","extent":{"kind":"fixed","count":3},"element":{"kind":"unsigned-integer","bits":8}})
    );
    let variants = array(&rows["Choice"]["variants"])?;
    assert_eq!(
        variants.iter().map(|v| &v["name"]).collect::<Vec<_>>(),
        ["Zero", "One", "More", "EmptyTuple", "EmptyRecord"]
    );
    assert_eq!(
        variants.iter().map(|v| &v["form"]).collect::<Vec<_>>(),
        ["unit", "tuple", "record", "tuple", "record"]
    );
    assert_eq!(
        array(&variants[1]["fields"])?
            .iter()
            .map(|v| v["position"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(variants[1]["fields"][0]["owner"], variants[1]["id"]);
    assert_eq!(
        rows["Packet"]["fields"][1]["type"],
        json!({"kind":"signed-integer","bits":64})
    );
    assert_eq!(rows["Bits"]["kind"], "union");
    assert!(
        !array(&g["gaps"])?
            .contains(&json!({"owner":fields["array"]["id"],"reason":"array-length"}))
    );
    let mut legacy = g.clone();
    legacy["vocabulary"] = json!("oddlang-data-shapes-v1");
    let fake = root.join("legacy.sh");
    fs::write(&fake, "#!/bin/sh\ncat \"$(dirname \"$0\")/legacy.json\"\n")?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755))?;
    fs::write(
        &path,
        format!(
            "{base}adapter={}\nadapterInputs=[\"legacy.json\"]\n",
            json!(fake)
        ),
    )?;
    let replay = |v: &Value| {
        write(
            &root.join("legacy.json"),
            &json!({"graph":v,"crateName":"observatory","compiler":PIN}),
        )
    };
    replay(&legacy)?;
    run("mislabeled", Some("array shape requires"))?;
    assert!(!root.join("mislabeled").exists());
    let frame = legacy["declarations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["name"] == "Frame")
        .unwrap();
    let field = frame["fields"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["name"] == "array")
        .unwrap();
    field["type"] = json!({"kind":"unresolved","reason":"array-length"});
    replay(&legacy)?;
    run("valid-old", None)?;
    assert_eq!(
        read(&root.join("valid-old/shapes.lock"))?["vocabulary"],
        "oddlang-data-shapes-v1"
    );
    assert_eq!(
        bytes,
        extract(
            &source.replace("struct Marker;", "\n\nstruct Marker;"),
            "reformat",
            false
        )?
        .0
    );
    let changed = source
        .replace(
            "word: u16,\n    shared: Arc<T>,",
            "shared: Arc<T>,\n    word: u32,",
        )
        .replace("Zero,\n    One(T, u128),", "One(T, u128),\n    Zero,")
        .replace("[u8; 3]", "[[u8; 0]; 2 + 2]");
    let (new_bytes, _, m) = extract(&changed, "mutation", true)?;
    assert_ne!(bytes, new_bytes);
    let new = indexed(&m["Frame"]["fields"])?;
    assert_eq!(new["word"]["type"]["bits"], 32);
    assert_eq!(new["word"]["id"], fields["word"]["id"]);
    assert_eq!(new["word"]["position"], 1);
    assert_eq!(
        new["array"]["type"]["extent"],
        json!({"kind":"fixed","count":4})
    );
    assert_eq!(
        new["array"]["type"]["element"]["extent"],
        json!({"kind":"fixed","count":0})
    );
    assert!(new.contains_key("extra"));
    assert_eq!(m["Choice"]["variants"][0]["id"], variants[1]["id"]);
    assert_eq!(m["Choice"]["variants"][0]["position"], 0);
    let (_, g, n) = extract(
        &fs::read_to_string(fixtures().join("network.rs"))?,
        "network",
        false,
    )?;
    assert_eq!(
        n.keys().collect::<Vec<_>>(),
        ["Arc", "Route", "Status", "Window"]
    );
    let r = indexed(&n["Route"]["fields"])?;
    assert_eq!(r["alias"]["type"]["id"], n["Arc"]["id"]);
    assert!(r["alias"]["type"].get("ownership").is_none());
    assert_eq!(r["cache"]["type"]["ownership"], "shared");
    assert_eq!(r["next"]["type"]["ownership"], "exclusive");
    let a = &n["Window"]["fields"][1]["type"];
    assert_eq!(a["element"]["id"], n["Window"]["parameters"][1]["id"]);
    assert_eq!(
        a["extent"],
        json!({"kind":"parameter","id":n["Window"]["parameters"][2]["id"]})
    );
    let args = array(&r["bytes"]["type"]["arguments"])?;
    assert_eq!(
        args.iter().map(|r| &r["name"]).collect::<Vec<_>>(),
        ["'a", "T", "N"]
    );
    assert_eq!(args[0]["value"]["id"], n["Route"]["parameters"][0]["id"]);
    assert_eq!(
        args[2]["value"],
        json!({"kind":"unresolved","reason":"constant-argument"})
    );
    assert_eq!(
        r["endpoint"]["type"]["lifetime"],
        json!({"kind":"static-lifetime"})
    );
    assert_eq!(r["offset"]["type"]["kind"], "target-integer");
    assert!(array(&g["gaps"])?.contains(
        &json!({"owner":n["Status"]["variants"][0]["id"],"reason":"explicit-discriminant"})
    ));
    Ok(())
}
