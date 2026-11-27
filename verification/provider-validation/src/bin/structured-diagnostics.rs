//! Compare diagnostics from two source revisions with their graph-only replacement.
use provider_validation::{Result, cli, pin, read, sha, subprocess, write};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn run(command: &mut Command, error: Option<&str>) -> Result<Output> {
    let out = subprocess::run(
        command
            .env("RUSTUP_TOOLCHAIN", "1.97.1")
            .env_remove("CARGO_TARGET_DIR"),
        None,
    )?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    match error {
        Some(msg) if !out.status.success() && stderr.contains(msg) => Ok(out),
        None if out.status.success() => Ok(out),
        _ => Err(format!("{command:?}: {} {stderr}", out.status).into()),
    }
}
fn caller(root: &Path, library: &Path, fixture: &Path) -> Result<Vec<u8>> {
    fs::create_dir_all(root.join("src"))?;
    fs::copy(fixture.join("caller.rs"), root.join("src/main.rs"))?;
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname='station-caller'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nstation-observer={{path={}}}\n",
            serde_json::to_string(library)?
        ),
    )?;
    run(
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(root),
        None,
    )?;
    run(
        Command::new("cargo")
            .args(["build", "--offline", "--locked"])
            .current_dir(root),
        None,
    )?;
    let bin = root.join("target/debug/station-caller");
    let normal = run(&mut Command::new(&bin), None)?;
    let (reader, mut writer) = io::pipe()?;
    drop(reader);
    assert_eq!(
        writer.write_all(b"probe").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    let closed = subprocess::run(&mut Command::new(bin), Some(writer))?;
    assert!(closed.status.success());
    assert_eq!(closed.stdout, b"after\n");
    assert_eq!(closed.stdout, normal.stdout);
    Ok(normal.stderr)
}
fn project(
    cli: &Path,
    root: &Path,
    fixture: &Path,
    name: &str,
    source: &str,
) -> Result<(Value, String)> {
    let reference = root.join("reference");
    fs::create_dir_all(reference.join("src"))?;
    for file in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(fixture.join(file), reference.join(file))?;
    }
    fs::write(reference.join("src/records.rs"), source)?;
    fs::write(reference.join("src/lib.rs"), "pub mod records;\n")?;
    let inputs = root.join("portable");
    fs::create_dir(&inputs)?;
    for dest in ["graph.json", "again.json"] {
        run(
            Command::new(cli)
                .args(["recover", "structured-diagnostics"])
                .arg(reference.join("src/records.rs"))
                .arg(inputs.join(dest)),
            None,
        )?;
    }
    assert_eq!(
        fs::read(inputs.join("graph.json"))?,
        fs::read(inputs.join("again.json"))?
    );
    write(
        &inputs.join("binding.json"),
        &json!({"sink":"stderr","borrow":"static"}),
    )?;
    fs::copy(fixture.join("Cargo.lock"), inputs.join("Cargo.lock"))?;
    let mut manifest = json!({"format":"oddlang-native-package-v1","name":"station-observer","version":"3.4.5","edition":"2024","cargoLock":pin(&inputs,"Cargo.lock")?,"dependencies":{"serde_json":{"version":"1.0.154"}},
        "components":[{"kind":"structured-diagnostics","module":"records","visibility":"public","rootReexports":[],"graph":pin(&inputs,"graph.json")?,"overlay":pin(&inputs,"binding.json")?,"reexports":["begin","observation","mark","end"]}]});
    write(&inputs.join("package.json"), &manifest)?;
    run(
        Command::new(cli).current_dir(&inputs).args([
            "build",
            "native-package",
            "rust",
            "package.json",
            "../candidate",
        ]),
        None,
    )?;
    run(
        Command::new(cli).current_dir(&inputs).args([
            "build",
            "structured-diagnostics",
            "rust",
            "graph.json",
            "binding.json",
            "../projection",
        ]),
        None,
    )?;
    assert_eq!(
        read(&root.join("candidate/package.lock"))?["components"][0]["sourceSha256"],
        sha(&root.join("projection/diagnostics.rs"))?
    );
    let original = caller(&root.join("reference-caller"), &reference, fixture)?;
    let emitted = caller(
        &root.join("projected-caller"),
        &root.join("candidate"),
        fixture,
    )?;
    assert_eq!(original, emitted);
    let text = String::from_utf8(emitted)?;
    let rows = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut canonical = String::new();
    for row in &rows {
        canonical.push_str(&serde_json::to_string(row)?);
        canonical.push('\n');
    }
    assert_eq!(text, canonical);
    assert_eq!(rows.len(), if name == "original" { 4 } else { 5 });
    let startup = rows
        .iter()
        .filter(|r| r["event"] == "startup")
        .collect::<Vec<_>>();
    assert_eq!(startup.len(), 2);
    assert!(
        startup
            .iter()
            .all(|r| r["version"] == "3.4.5" && r["code"] == 7)
    );
    assert!(
        rows.iter()
            .any(|r| r["point"] == "númeró" && r["reading"] == "control\u{1}\n")
    );
    manifest["dependencies"]
        .as_object_mut()
        .unwrap()
        .remove("serde_json");
    write(&inputs.join("package.json"), &manifest)?;
    run(
        Command::new(cli).current_dir(&inputs).args([
            "build",
            "native-package",
            "rust",
            "package.json",
            "../bad-package",
        ]),
        Some("pinned serde_json dependency"),
    )?;
    Ok((read(&inputs.join("graph.json"))?, text))
}
fn main() -> Result {
    let cli = cli()?;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../structured-diagnostics");
    let source = fs::read_to_string(fixture.join("reference.rs"))?;
    let scratch = tempfile::tempdir()?;
    let root = scratch.path();
    let (graph, baseline) = project(&cli, &root.join("original"), &fixture, "original", &source)?;
    let (mutant, changed) = project(
        &cli,
        &root.join("mutated"),
        &fixture,
        "mutated",
        &source
            .replace("AtomicBool::new(false)", "AtomicBool::new(true)")
            .replace(r#""initial\r\t""#, "\"revised\""),
    )?;
    assert_ne!(graph, mutant);
    assert_ne!(baseline, changed);
    assert!(
        !baseline.contains(r#""point":"before""#)
            && changed.contains(r#""point":"before""#)
            && changed.contains(r#""reading":"revised""#)
    );
    for (invalid, msg) in [
        (
            source.replacen("Ordering::Relaxed", "Ordering::SeqCst", 1),
            "unsupported diagnostic statement",
        ),
        (
            source.replace(r#""code": 7"#, r#""code": 7,"code": 8"#),
            "duplicate record field",
        ),
        (
            source.replace("fn observation(point:", "fn observation(observation:"),
            "expected read-only text parameter",
        ),
        (
            source
                .replace("observation(point:", "observation(null:")
                .replace(r#""point": point"#, r#""point": null"#),
            "invalid or duplicate parameter null",
        ),
        (
            source.replace(r#"observation(new, "initial"#, r#"mark(new, "initial"#),
            "earlier compatible text function",
        ),
    ] {
        fs::write(root.join("invalid.rs"), invalid)?;
        run(
            Command::new(&cli).current_dir(root).args([
                "recover",
                "structured-diagnostics",
                "invalid.rs",
                "invalid.json",
            ]),
            Some(msg),
        )?;
    }
    let invalid_graph = |raw: String, msg: &str| -> Result {
        fs::write(root.join("invalid.json"), raw)?;
        run(
            Command::new(&cli).current_dir(root).args([
                "build",
                "structured-diagnostics",
                "rust",
                "invalid.json",
                "original/portable/binding.json",
                "invalid-projection",
            ]),
            Some(msg),
        )?;
        Ok(())
    };
    let encoded = serde_json::to_string(&graph)?;
    let field = r#""code":{"kind":"unsigned","value":7}"#;
    assert!(encoded.contains(field));
    invalid_graph(
        encoded.replace(
            field,
            &format!(r#"{field},"code":{{"kind":"unsigned","value":8}}"#),
        ),
        "duplicate record field code",
    )?;
    let mut shadow = graph.clone();
    shadow["functions"][1]["parameters"][0]["name"] = "null".into();
    invalid_graph(
        serde_json::to_string(&shadow)?,
        "invalid or duplicate parameter null",
    )?;
    let mut action = graph;
    action["functions"][2]["actions"][0]["target"] = "new".into();
    invalid_graph(
        serde_json::to_string(&action)?,
        "expected writable text parameter",
    )
}
