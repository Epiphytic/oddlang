use provider_validation::{Result, cli, command, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const SOURCE: &str = r#"
#[derive(Debug, Default)]
pub struct Contour {
    pub label: String,
    pub cells: usize,
}
"#;

fn recover(cli: &Path, work: &Path, source: &str, failure: Option<&str>) -> Result {
    fs::create_dir(work)?;
    fs::write(work.join("source.rs"), source)?;
    write(
        &work.join("selection.json"),
        &json!({"format":"oddlang-rust-root-record-selection-v1",
        "sourceSha256":sha(&work.join("source.rs"))?, "id":"urn:terrain:root","records":{"Contour":"urn:terrain:contour"}}),
    )?;
    invoke(
        cli,
        work,
        &[
            "recover",
            "public-root-records",
            "source.rs",
            "selection.json",
        ],
        "model",
        failure,
    )?;
    if failure.is_none() {
        invoke(
            cli,
            work,
            &[
                "build",
                "public-records",
                "rust",
                "model/graph.json",
                "model/rust.json",
            ],
            "projection",
            None,
        )?;
    }
    Ok(())
}

fn records(cli: &Path, root: &Path) -> Result {
    for (kind, native) in [("pointer-unsigned", "usize"), ("u32", "u32")] {
        let work = root.join(kind);
        let source = SOURCE.replace("usize", native);
        recover(cli, &work, &source, None)?;
        let graph = read(&work.join("model/graph.json"))?;
        let field = &graph["modules"][0]["records"][0]["fields"][1];
        assert_eq!(field["type"], kind);
        assert_eq!(field["wire"], "cells");
        let probe = format!(
            r#"
fn main() {{
    let a = Contour::default();
    let zero: ::core::primitive::{native} = a.cells;
    assert_eq!(zero, 0);
    let max = Contour {{ label: "ridge".into(), cells: ::core::primitive::{native}::MAX }};
    println!("{{}}:{{}}:{{}}", max.label, max.cells, ::core::primitive::{native}::BITS);
}}
"#
        );
        let mut outputs = Vec::new();
        for (label, implementation) in [
            ("original", source.as_str()),
            ("generated", r#"include!("projection/records.rs");"#),
        ] {
            fs::write(work.join("probe.rs"), format!("{implementation}{probe}"))?;
            command(
                &mut Command::new("rustc"),
                &work,
                &["--edition=2021", "probe.rs", "-o", label],
                None,
            )?;
            outputs.push(command(
                &mut Command::new(work.join(label)),
                &work,
                &[],
                None,
            )?);
        }
        assert_eq!(outputs[0], outputs[1]);
    }
    recover(
        cli,
        &root.join("shadow"),
        &format!("type r#usize = u32;\n{SOURCE}"),
        Some("shadowed public record"),
    )?;
    recover(
        cli,
        &root.join("cfg"),
        &SOURCE.replace(
            "pub cells",
            r#"#[cfg(target_pointer_width = "64")] pub cells"#,
        ),
        Some("unsupported non-public or attributed field"),
    )
}

fn recover_module(
    cli: &Path,
    root: &Path,
    selection: &mut Value,
    prefix: &str,
    out: &str,
    failure: Option<&str>,
) -> Result {
    let module = root.join("src/contour.rs");
    fs::write(&module, format!("{prefix}{SOURCE}"))?;
    selection["modules"][0]["sha256"] = json!(sha(&module)?);
    write(&root.join("selection.json"), selection)?;
    invoke(
        cli,
        root,
        &["recover", "public-records", "src/lib.rs", "selection.json"],
        out,
        failure,
    )
}

fn modules(cli: &Path, root: &Path) -> Result {
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub mod contour;\n")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"terrain\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
    )?;
    let mut selection = json!({"format":"oddlang-rust-public-record-selection-v1","libSha256":sha(&root.join("src/lib.rs"))?,
        "cargoSha256":sha(&root.join("Cargo.toml"))?,"modules":[{"id":"urn:terrain:module","name":"contour","records":{"Contour":"urn:terrain:contour"}}]});
    let glob = "use catalog::shapes::*;\n";
    let diagnostic = Some("unsupported public record glob source");
    recover_module(cli, root, &mut selection, glob, "strict", diagnostic)?;
    selection["format"] = json!("oddlang-rust-public-record-candidate-selection-v1");
    recover_module(cli, root, &mut selection, glob, "candidate", None)?;
    let lock = read(&root.join("candidate/recovery.lock"))?;
    assert_eq!(
        lock["unresolvedGlobBindings"],
        json!({"contour":["catalog::shapes::*"]})
    );
    assert!(
        lock["gap"]
            .as_str()
            .is_some_and(|s| s.contains("independent native compilation"))
    );
    recover_module(cli, root, &mut selection, glob, "repeat", None)?;
    assert_eq!(
        fs::read(root.join("candidate/recovery.lock"))?,
        fs::read(root.join("repeat/recovery.lock"))?
    );
    for (out, prefix) in [
        ("local-shadow", "use crate::catalog::*;\n".to_owned()),
        (
            "inline-module",
            format!("mod catalog {{ pub mod shapes {{ pub type usize = u32; }} }}\n{glob}"),
        ),
        ("named-local-import", format!("use crate::catalog;\n{glob}")),
        ("conditional-glob", format!("#[cfg(unix)] {glob}")),
    ] {
        recover_module(cli, root, &mut selection, &prefix, out, diagnostic)?;
    }
    recover_module(
        cli,
        root,
        &mut selection,
        "mod catalog {}\nuse ::catalog::shapes::*;\n",
        "absolute",
        None,
    )?;
    assert_eq!(
        read(&root.join("absolute/recovery.lock"))?["unresolvedGlobBindings"],
        json!({"contour":["::catalog::shapes::*"]})
    );
    let child = root.join("src/local.rs");
    fs::write(
        &child,
        "pub mod catalog { pub mod shapes { pub type usize = u32; } }\n",
    )?;
    selection["modules"][0]["scopeGlobs"] = json!({"local":sha(&child)?});
    recover_module(
        cli,
        root,
        &mut selection,
        &format!("{glob}#[path = \"local.rs\"] mod local; use local::*;\n"),
        "pinned-child-prefix",
        diagnostic,
    )
}
fn main() -> Result {
    let compiler = cli()?;
    let work = tempfile::tempdir()?;
    records(&compiler, work.path())?;
    modules(&compiler, work.path())?;
    println!(
        "Pointer-width records, fixed-width mutation, native observations and module glob controls passed"
    );
    Ok(())
}
