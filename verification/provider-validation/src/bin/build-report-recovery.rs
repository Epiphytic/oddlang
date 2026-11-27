//! Recover the ordered fields, aliases, conversion chain, and independently expected values.
use provider_validation::{Result, cli, invoke, pin, read, write};
use serde_json::{Value, json};
use std::{fs, path::Path};

const SOURCE: &str = include_str!("../../fixtures/build-report/record.rs");
const ROOT: &str = "mod compile; mod format;\n";
const MACRO: &str = "macro_rules! env { ($value:literal) => { \"injected\" }; }";
fn select(root: &Path) -> Result<Value> {
    let paths = [
        "rover/lib.rs",
        "rover/compile.rs",
        "rover/format.rs",
        "library/lib.rs",
        "library/stored.rs",
    ];
    let pins = paths
        .into_iter()
        .map(|p| pin(root, p))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"format":"oddlang-build-report-recovery-v1","id":"urn:rover:about","crate":"rover","module":["compile"],"record":"Identification",
        "constructor":"identification","targetEnv":"ROVER_MACHINE","revisionEnv":"ROVER_SOURCE","revision":"2".repeat(40),
        "roots":{"rover":"rover/lib.rs","library":"library/lib.rs"},"sources":pins}),
    )
}
fn main() -> Result {
    let cli = cli()?;
    let stage = tempfile::tempdir()?;
    let root = stage.path();
    fs::create_dir(root.join("rover"))?;
    fs::create_dir(root.join("library"))?;
    let record = root.join("rover/compile.rs");
    let ancestor = root.join("rover/lib.rs");
    let defs = root.join("library/stored.rs");
    fs::write(&record, SOURCE)?;
    fs::write(&ancestor, ROOT)?;
    fs::write(
        root.join("rover/format.rs"),
        "pub(crate) const EARLIEST: u8 = 5; pub(crate) const LATEST: u16 = 100;",
    )?;
    fs::write(
        root.join("library/lib.rs"),
        "mod stored; pub const PUBLIC: i64 = stored::NEGATIVE;",
    )?;
    fs::write(&defs, "pub(crate) const NEGATIVE: i64 = -3;")?;
    let run = |dest: &str, error: Option<&str>| -> Result {
        write(&root.join("selection.json"), &select(root)?)?;
        invoke(
            &cli,
            root,
            &["recover", "build-report", ".", "selection.json"],
            dest,
            error,
        )
    };
    run("recovered", None)?;
    let graph = read(&root.join("recovered/graph.json"))?;
    let fields = graph["fields"].as_array().ok_or("missing fields")?;
    assert_eq!(
        fields.iter().map(|f| &f["wire"]).collect::<Vec<_>>(),
        ["storage-ids", "origin", "envelope", "platform", "software"]
    );
    let first = &fields[0]["source"];
    assert_eq!(first["kind"], "unsigned-reference-list");
    assert_eq!(first["bits"], 16);
    let items = first["items"].as_array().ok_or("missing items")?;
    assert_eq!(
        items.iter().map(|v| &v["conversion"]).collect::<Vec<_>>(),
        ["wrap", "widen", "identity"]
    );
    assert_eq!(
        items[0]["chain"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("value"))
            .collect::<Vec<_>>(),
        [&json!(-3)]
    );
    assert_eq!(
        read(&root.join("recovered/observation.json"))?["values"],
        json!([[65533, 5, 100], "2".repeat(40), 9, null, null])
    );
    run("repeat", None)?;
    for file in ["graph.json", "observation.json"] {
        assert_eq!(
            fs::read(root.join("repeat").join(file))?,
            fs::read(root.join("recovered").join(file))?
        );
    }
    fs::write(&defs, "pub(crate) const NEGATIVE: i64 = 70000;")?;
    run("mutated", None)?;
    assert_eq!(
        read(&root.join("mutated/observation.json"))?["values"][0],
        json!([4464, 5, 100])
    );
    for (text, error) in [
        (
            "#[cfg(any())] pub(crate) const NEGATIVE: i64 = 70000;",
            "conditional",
        ),
        ("const NEGATIVE: i64 = 70000;", "inaccessible"),
        (
            "pub(crate) const NEGATIVE: é = 70000;",
            "unsupported integer type",
        ),
    ] {
        fs::write(&defs, text)?;
        run("rejected", Some(error))?;
    }
    fs::write(&defs, "pub(crate) const NEGATIVE: i64 = 7;")?;
    fs::write(&record, format!("use crate::format as library;\n{SOURCE}"))?;
    run("shadow", Some("shadow external crate"))?;
    for extra in [
        "type u16 = u32;",
        "mod u16 { pub fn from(_: u8) -> u16 { 42 } }",
        MACRO,
        "use custom::alternate as env;",
        "use helpers::*;",
        "use custom::alternate as r#env;",
    ] {
        fs::write(&record, format!("{extra}\n{SOURCE}"))?;
        run("reserved", Some("reserved"))?;
    }
    fs::write(&record, SOURCE)?;
    fs::write(&ancestor, format!("{MACRO}\n{ROOT}"))?;
    run("ancestor", Some("reserved"))?;
    fs::write(&ancestor, ROOT)?;
    fs::write(
        root.join("root-record.rs"),
        include_str!("../../fixtures/build-report/root-record.rs"),
    )?;
    fs::write(
        root.join("helper.rs"),
        "#[macro_export]\nmacro_rules! env { ($name:literal) => { \"sentinel\" }; }\n",
    )?;
    let mut config = select(root)?;
    config["roots"] = json!({"rover":"root-record.rs"});
    config["module"] = json!([]);
    config["sources"] = json!([pin(root, "root-record.rs")?]);
    write(&root.join("selection.json"), &config)?;
    invoke(
        &cli,
        root,
        &["recover", "build-report", ".", "selection.json"],
        "root-output",
        Some("submodule"),
    )?;
    println!(
        "Build-report recovery: ordered fields, numeric conversions, mutation, repeatability and source-scope rejections passed"
    );
    Ok(())
}
