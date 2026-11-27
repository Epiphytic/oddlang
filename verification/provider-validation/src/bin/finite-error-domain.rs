//! Recover two source variants, then check generated native callers and rejection cases.
use provider_validation::{Result, cli, command, invoke, read, sha, write};
use serde_json::json;
use std::{fs, path::Path, process::Command};

const SOURCE: &str = include_str!("../../fixtures/finite-error-domains/source.rs");
const CALLER: &str = include_str!("../../fixtures/finite-error-domains/probe.rs");

fn select(root: &Path, source: &str, error_name: &str) -> Result {
    let file = root.join("error.rs");
    fs::write(&file, source)?;
    write(
        &root.join("selection.json"),
        &json!({
            "format": "oddlang-finite-error-selection-v1", "id": "urn:sample:faults",
            "sourceSha256":sha(&file)?, "error":error_name, "result":"Outcome"
        }),
    )
}
fn build(
    cli: &Path,
    root: &Path,
    graph: &str,
    overlay: &str,
    out: &str,
    error: Option<&str>,
) -> Result {
    invoke(
        cli,
        root,
        &["build", "finite-error-domain", "rust", graph, overlay],
        out,
        error,
    )
}
fn compile(root: &Path, caller: &str, error: Option<&str>) -> Result {
    fs::write(root.join("caller.rs"), caller)?;
    command(
        &mut Command::new("rustc"),
        root,
        &["--edition=2021", "caller.rs", "-o", "probe"],
        error,
    )?;
    Ok(())
}
fn main() -> Result {
    let cli = cli()?;
    let stage = tempfile::tempdir()?;
    let root = stage.path();
    let recover = [
        "recover",
        "finite-error-domain",
        "error.rs",
        "selection.json",
    ];
    for (name, label) in [("baseline", "absent"), ("mutated", "missing")] {
        select(
            root,
            &SOURCE.replace("\"absent\"", &format!("{label:?}")),
            "Fault",
        )?;
        invoke(&cli, root, &recover, name, None)?;
        let projected = format!("{name}-rust");
        build(
            &cli,
            root,
            &format!("{name}/graph.json"),
            &format!("{name}/rust.json"),
            &projected,
            None,
        )?;
        fs::copy(
            root.join(projected).join("errors.rs"),
            root.join("errors.rs"),
        )?;
        compile(root, &CALLER.replace("ABSENT", label), None)?;
        command(&mut Command::new(root.join("probe")), root, &[], None)?;
    }
    assert_ne!(
        fs::read(root.join("baseline/graph.json"))?,
        fs::read(root.join("mutated/graph.json"))?
    );
    let mut graph = read(&root.join("baseline/graph.json"))?;
    let variants = graph["variants"]
        .as_object_mut()
        .ok_or("missing variants")?;
    let (_, measurement) = variants
        .iter_mut()
        .find(|(name, _)| name.ends_with(":Measurement"))
        .ok_or("missing Measurement")?;
    measurement["source"] = json!(true);
    write(&root.join("invalid-graph.json"), &graph)?;
    build(
        &cli,
        root,
        "invalid-graph.json",
        "baseline/rust.json",
        "rejected",
        Some("requires an I/O error payload"),
    )?;
    let mut overlay = read(&root.join("baseline/rust.json"))?;
    let category = overlay["categoryTypes"]
        .as_object_mut()
        .and_then(|types| types.values_mut().next())
        .ok_or("missing category types")?;
    *category
        .as_array_mut()
        .and_then(|path| path.last_mut())
        .ok_or("missing category path")? = json!("Reading; panic!()");
    write(&root.join("invalid-rust.json"), &overlay)?;
    build(
        &cli,
        root,
        "baseline/graph.json",
        "invalid-rust.json",
        "rejected",
        Some("invalid native category type"),
    )?;
    for (from, to) in [
        ("Broken => \"unreadable\"", "_ => \"unreadable\""),
        ("match code", "match other"),
        ("formatter(.0)", "formatter(.1)"),
    ] {
        select(root, &SOURCE.replace(from, to), "Fault")?;
        invoke(&cli, root, &recover, "rejected", Some("category"))?;
    }
    compile(
        root,
        &CALLER.replace("Missing, Broken", "Missing, Broken, NewCase"),
        Some("non-exhaustive patterns"),
    )?;
    select(
        root,
        &SOURCE
            .replace("pub enum Fault", "pub enum T")
            .replace("Result<T, Fault>", "Result<T, T>"),
        "T",
    )?;
    invoke(
        &cli,
        root,
        &recover,
        "rejected",
        Some("result alias parameter shadows"),
    )?;
    println!(
        "Finite error domain: baseline and mutated native callers, invalid projection and recovery cases, and exhaustiveness passed"
    );
    Ok(())
}
