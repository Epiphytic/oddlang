//! Replay independent Rust and TypeScript choice qualification cases.
use provider_validation::{Result, command, invoke, read, sha, write};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
macro_rules! fixture {
    ($file:literal) => {
        include_str!(concat!("../../../fixtures/choices/", $file))
    };
}
mod native;
mod terrain;
mod typescript;

fn selection(
    cli: &Path,
    root: &Path,
    source: &str,
    id: &str,
    name: &str,
    version: &str,
    output: &str,
    error: Option<&str>,
) -> Result {
    fs::write(root.join("choice.rs"), source)?;
    write(
        &root.join("selection.json"),
        &json!({"format":format!("oddlang-plain-choice-selection-{version}"),"id":id,"name":name,"sourceSha256":sha(&root.join("choice.rs"))?}),
    )?;
    invoke(
        cli,
        root,
        &["recover", "plain-choice", "choice.rs", "selection.json"],
        output,
        error,
    )
}
fn project(
    cli: &Path,
    root: &Path,
    language: &str,
    graph: &str,
    binding: &str,
    dest: &str,
    error: Option<&str>,
) -> Result {
    invoke(
        cli,
        root,
        &["build", "checked-values", language, graph, binding],
        dest,
        error,
    )
}
fn run(root: &Path, tool: &str, args: &[&str], error: Option<&str>) -> Result<Vec<u8>> {
    command(&mut Command::new(tool), root, args, error)
}
fn library(root: &Path, name: &str, file: &str, edition: &str) -> Result {
    run(
        root,
        "rustc",
        &[
            edition,
            "--crate-name",
            name,
            "--crate-type",
            "rlib",
            file,
            "-o",
            &format!("lib{name}.rlib"),
        ],
        None,
    )?;
    Ok(())
}
fn caller(root: &Path, name: &str, source: &str, edition: &str, error: Option<&str>) -> Result {
    fs::write(root.join("caller.rs"), source)?;
    run(
        root,
        "rustc",
        &[
            edition,
            "--extern",
            &format!("{name}=lib{name}.rlib"),
            "caller.rs",
            "-o",
            "caller",
        ],
        error,
    )?;
    Ok(())
}
fn trait_check(
    root: &Path,
    name: &str,
    ty: &str,
    bound: &str,
    expected: bool,
    edition: &str,
) -> Result {
    fs::write(
        root.join("caller.rs"),
        format!("fn require<T:{bound}>(){{}} fn main(){{require::<{name}::{ty}>();}}"),
    )?;
    let output = Command::new("rustc")
        .env("RUSTUP_TOOLCHAIN", "1.97.1")
        .current_dir(root)
        .args([
            edition,
            "--extern",
            &format!("{name}=lib{name}.rlib"),
            "--error-format=json",
            "caller.rs",
            "-o",
            "caller",
        ])
        .output()?;
    if expected {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        assert!(!output.status.success());
        let mut codes = Vec::new();
        for line in std::str::from_utf8(&output.stderr)?.lines() {
            let row: Value = serde_json::from_str(line)?;
            if let Some(code) = row["code"]["code"].as_str() {
                codes.push(code.to_string());
            }
        }
        assert_eq!(codes, ["E0277"]);
    }
    Ok(())
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [case, cli, extra @ ..] = args.as_slice() else {
        return Err("usage: qualify-choices CASE CLI [COMPARISON_CLI]".into());
    };
    let cli = Path::new(cli).canonicalize()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    match (case.as_str(), extra) {
        ("verify", []) => native::verify(&cli, root),
        ("equality", []) => native::equality(&cli, root),
        ("capabilities", []) => native::capabilities(&cli, root),
        ("legacy-bytes", [new]) => native::legacy(&cli, &PathBuf::from(new).canonicalize()?, root),
        ("typescript", []) => typescript::check(&cli, root),
        ("typescript-subset", []) => typescript::subset(&cli, None, root),
        ("typescript-subset", [older]) => {
            typescript::subset(&cli, Some(&PathBuf::from(older).canonicalize()?), root)
        }
        ("serde-string", []) => terrain::check(&cli, root),
        _ => Err(format!("unknown case or invalid arguments: {args:?}").into()),
    }
}
