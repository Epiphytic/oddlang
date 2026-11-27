//! Interpret recovered fields independently and compare bytes with the native radio writer.
use provider_validation::{Result, cli, command, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{fmt::Write, fs, path::Path, process::Command};
const SOURCE: &str = include_str!("../../../record-writer/telemetry.rs");
fn recover(
    cli: &Path,
    root: &Path,
    source: &str,
    bad_pin: bool,
    rejection: bool,
) -> Result<Option<Value>> {
    fs::create_dir(root)?;
    let file = root.join("input.rs");
    fs::write(&file, source)?;
    let digest = if bad_pin { "0".repeat(64) } else { sha(&file)? };
    write(
        &root.join("selection.json"),
        &json!({"sourceSha256":digest,"record":"Reading","writer":"encode","owner":"urn:radio:reading","tagTypes":{"Band":"urn:radio:band"}}),
    )?;
    invoke(
        cli,
        root,
        &["recover", "record-writer", "input.rs", "selection.json"],
        "output",
        rejection.then_some(""),
    )?;
    if rejection {
        return Ok(None);
    }
    let graph = root.join("output/graph.json");
    assert_eq!(
        read(&root.join("output/observation.json"))?["graphSha256"],
        sha(&graph)?
    );
    Ok(Some(read(&graph)?))
}
fn encode(graph: &Value, tag: u8) -> Result<String> {
    let mut bytes = Vec::new();
    for slot in graph["fields"].as_array().ok_or("missing fields")? {
        let id = slot["field"].as_str().ok_or("missing field ID")?;
        let enc = slot["encoding"].as_str().ok_or("missing encoding")?;
        match (id.strip_prefix("urn:radio:reading:field:"), enc) {
            (Some("sequence"), "unsigned-be") => {
                assert_eq!(graph["fieldTypes"][id]["bits"], 64);
                bytes.extend(257u64.to_be_bytes());
            }
            (Some("station"), "fixed-bytes") => {
                assert_eq!(graph["fieldTypes"][id]["bytes"], 2);
                bytes.extend([9, 0]);
            }
            (Some("band"), "external-tag-u8") => bytes.push(tag),
            (Some("caption"), "u32-be-length-prefixed-bytes") => {
                let caption = [0, 255, 65];
                bytes.extend((caption.len() as u32).to_be_bytes());
                bytes.extend(caption);
            }
            (Some("powered"), "bool-u8") => bytes.push(1),
            _ => return Err(format!("unexpected field or encoding: {id}, {enc}").into()),
        }
    }
    let mut hex = String::new();
    for byte in bytes {
        write!(hex, "{byte:02x}")?;
    }
    Ok(hex)
}
fn compare(root: &Path, g: &Value) -> Result {
    command(
        &mut Command::new("rustc"),
        root,
        &["--edition=2021", "input.rs", "-o", "original"],
        None,
    )?;
    let out = command(&mut Command::new(root.join("original")), root, &[], None)?;
    assert_eq!(
        String::from_utf8(out)?,
        format!("{}\n{}\n", encode(g, 3)?, encode(g, 9)?)
    );
    Ok(())
}
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let model = |name: &str, source: &str| -> Result<Value> {
        recover(&cli, &root.join(name), source, false, false)?.ok_or_else(|| "missing graph".into())
    };
    let graph = model("base", SOURCE)?;
    compare(&root.join("base"), &graph)?;
    let fields = graph["fields"].as_array().ok_or("missing fields")?;
    let encodings: Vec<_> = fields.iter().map(|f| &f["encoding"]).collect();
    assert_eq!(
        encodings,
        [
            "unsigned-be",
            "fixed-bytes",
            "external-tag-u8",
            "u32-be-length-prefixed-bytes",
            "bool-u8"
        ]
    );
    assert_eq!(
        model(
            "renamed",
            &SOURCE.replace("packet", "blob").replace("target", "out")
        )?,
        graph
    );
    let (a, b) = ("counter(sink, row.sequence);", "sink.extend(row.station);");
    let swapped = model(
        "swap",
        &SOURCE.replace(a, "TEMP").replace(b, a).replace("TEMP", b),
    )?;
    compare(&root.join("swap"), &swapped)?;
    assert_eq!(swapped["fields"][0], fields[1]);
    assert_eq!(swapped["fields"][1], fields[0]);
    assert_eq!(
        model("raw", &SOURCE.replace("powered", "r#type"))?["fields"][4]["field"],
        "urn:radio:reading:field:type"
    );
    assert_eq!(
        graph["externalTags"]["urn:radio:band"],
        json!({"urn:radio:band:Low":3,"urn:radio:band:High":9})
    );
    for (i, (old, new)) in [
        (
            "enum Band",
            "macro_rules! shape { () => { type u64 = u32; } } shape!(); enum Band",
        ),
        ("enum Band", "#[another::inject] enum Band"),
        (
            "enum Band",
            "use another::rewrite as serde; #[serde] enum Band",
        ),
        ("enum Band", "type r#u64 = u32; enum Band"),
        ("enum Band", "use another::Bytes as r#Vec; enum Band"),
        ("High = 9", "High = 300"),
        ("to_be_bytes", "to_le_bytes"),
        (
            "target.extend(data);",
            "target.extend(data); target.push(0);",
        ),
        (a, "if row.powered { counter(sink, row.sequence); }"),
        ("fn packet(", "#[cfg(unix)]\nfn packet("),
        ("struct Reading", "use custom::Vec;\nstruct Reading"),
        (b, "sink.extend(row.station); unknown(sink);"),
    ]
    .iter()
    .enumerate()
    {
        recover(
            &cli,
            &root.join(format!("invalid-{i}")),
            &SOURCE.replace(old, new),
            false,
            true,
        )?;
    }
    recover(&cli, &root.join("pin"), SOURCE, true, true)?;
    println!(
        "Source-derived byte order matched an independent native writer, including the reordered writer."
    );
    Ok(())
}
