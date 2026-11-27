use super::*;
use sha2::{Digest, Sha256};

fn recover(
    cli: &Path,
    root: &Path,
    source: &str,
    name: &str,
    error: Option<&str>,
    stale: bool,
) -> Result<PathBuf> {
    let dir = root.join(name);
    fs::create_dir(&dir)?;
    fs::write(dir.join("source.rs"), source)?;
    let bytes = if stale {
        fixture!("terrain.rs")
    } else {
        source
    };
    write(
        &dir.join("selection.json"),
        &json!({"format":"oddlang-rust-serde-string-choice-selection-v1","id":"urn:terrain:choice-domain",
        "enumId":"urn:terrain:terrain","name":"Terrain","variants":{"Marsh":"urn:terrain:marsh","Granite":"urn:terrain:granite"},
        "sourceSha256":format!("{:x}",Sha256::digest(bytes.as_bytes()))}),
    )?;
    invoke(
        cli,
        &dir,
        &[
            "recover",
            "serde-string-choice",
            "source.rs",
            "selection.json",
        ],
        "recovered",
        error,
    )?;
    Ok(dir)
}
fn native(
    root: &Path,
    include: &str,
    values: &[(String, String)],
    copy: bool,
    target: &Path,
) -> Result<Vec<u8>> {
    fs::create_dir_all(root.join("src"))?;
    let variants = values
        .iter()
        .map(|(name, _)| format!("Terrain::{name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let wires = values
        .iter()
        .map(|(_, wire)| json!(json!(wire).to_string()).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let body = fixture!("terrain-caller.rs.in")
        .replace("@VARIANTS@", &variants)
        .replace("@WIRES@", &wires);
    fs::write(root.join("src/main.rs"), format!("{include}\n{body}"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='terrain-recovery'\nversion='0.0.0'\nedition='2024'\n[dependencies]\nserde={version='=1.0.229',features=['derive']}\nserde_json='=1.0.154'\n",
    )?;
    let cargo = |args: &[&str], error: Option<&str>| {
        command(
            Command::new("cargo").env("CARGO_TARGET_DIR", target),
            root,
            args,
            error,
        )
    };
    cargo(&["generate-lockfile", "--offline"], None)?;
    let output = cargo(&["run", "--offline", "--locked"], None)?;
    fs::write(
        root.join("src/main.rs"),
        format!("{include}\nfn main(){{fn copy<T:Copy>(){{}} copy::<Terrain>();}}"),
    )?;
    cargo(
        &["check", "--offline", "--locked"],
        (!copy).then_some("Terrain: Copy"),
    )?;
    Ok(output)
}
type TerrainCase = (String, String, Vec<(String, String)>, bool);
pub fn check(cli: &Path, root: &Path) -> Result {
    let cases: Vec<TerrainCase> = serde_json::from_str(fixture!("terrain-cases.json"))?;
    for (label, source, values, copy) in cases {
        let dir = recover(cli, root, &source, &label, None, false)?;
        let repeat = recover(cli, root, &source, &format!("{label}-repeat"), None, false)?;
        for file in ["graph.json", "rust.json", "recovery.lock"] {
            assert_eq!(
                fs::read(dir.join("recovered").join(file))?,
                fs::read(repeat.join("recovered").join(file))?
            );
        }
        let graph = read(&dir.join("recovered/graph.json"))?;
        let item = &graph["stringEnums"][0];
        let wires = item["variants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["wire"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            wires,
            values
                .iter()
                .map(|(_, wire)| wire.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(item["copy"].as_bool().unwrap_or(true), copy);
        invoke(
            cli,
            &dir,
            &[
                "build",
                "shared-declarations",
                "rust",
                "recovered/graph.json",
                "recovered/rust.json",
            ],
            "projected",
            None,
        )?;
        let generated = fs::read_to_string(dir.join("projected/shared.rs"))?;
        let target = root.join("target");
        assert_eq!(
            native(&dir.join("orig"), &source, &values, copy, &target)?,
            native(&dir.join("emitted"), &generated, &values, copy, &target)?
        );
    }
    let invalid: Vec<(String, String, String)> =
        serde_json::from_str(fixture!("terrain-rejections.json"))?;
    for (source, label, error) in invalid {
        recover(cli, root, &source, &label, Some(&error), label == "stale")?;
    }
    println!(
        "Pinned Serde string choices, native capabilities, spelling/order mutation and rejection controls pass."
    );
    Ok(())
}
