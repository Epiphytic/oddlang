//! Compile independent frame and record projections into the native split-input observer.
use provider_validation::{Result, cli, command, invoke, read, write};
use std::{fs, path::Path, process::Command};
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../binary-frame");
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let fixture = Path::new(FIXTURE);
    for (kind, graph, binding, output) in [
        ("binary-frame", "graph", "binding", "src"),
        ("binary-record", "device", "overlay", "inner"),
        ("binary-record", "existing", "existing_native", "bound"),
    ] {
        fs::copy(
            fixture.join(format!("{graph}.json")),
            root.join("graph.json"),
        )?;
        fs::copy(
            fixture.join(format!("{binding}.json")),
            root.join("rust.json"),
        )?;
        invoke(
            &cli,
            root,
            &["build", kind, "rust", "graph.json", "rust.json"],
            output,
            None,
        )?;
    }
    fs::copy(root.join("inner/record.rs"), root.join("src/record.rs"))?;
    fs::copy(root.join("bound/record.rs"), root.join("src/bound.rs"))?;
    fs::copy(fixture.join("observer.rs"), root.join("src/main.rs"))?;
    command(
        &mut Command::new("rustc"),
        root,
        &["--edition=2024", "src/main.rs", "-o", "observer"],
        None,
    )?;
    command(&mut Command::new(root.join("observer")), root, &[], None)?;
    let mut graph = read(&fixture.join("graph.json"))?;
    graph["cases"]["urn:meter:samples"]["encodeGuards"]
        .as_array_mut()
        .ok_or("missing guards")?
        .clear();
    write(&root.join("graph.json"), &graph)?;
    fs::copy(fixture.join("binding.json"), root.join("rust.json"))?;
    invoke(
        &cli,
        root,
        &["build", "binary-frame", "rust", "graph.json", "rust.json"],
        "rejected",
        Some("missing frame encode bound"),
    )?;
    println!("Tagged binary cases, nested checks and incremental poisoning pass.");
    Ok(())
}
