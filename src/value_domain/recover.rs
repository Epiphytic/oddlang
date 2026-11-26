//! Derive a candidate interval and named errors through the selected native public API.
use super::{Binding, Result, digest};
use oxigraph::model::{Literal, NamedNode};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    source_revision: String,
    cargo_lock_sha256: String,
    compiler_commit: String,
    package: String,
    bits: u8,
    range_error: String,
    api: Binding,
}

fn output(cmd: &mut Command) -> Result<Vec<u8>> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "{} failed: {}",
            cmd.get_program().to_string_lossy(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

fn clean(root: &Path, revision: &str) -> Result<()> {
    let git = |args: &[&str]| output(Command::new("git").current_dir(root).args(args));
    if String::from_utf8_lossy(&git(&["rev-parse", "HEAD"])?).trim() != revision
        || !git(&["status", "--porcelain", "--untracked-files=all"])?.is_empty()
    {
        return Err("original checkout does not match the clean revision".into());
    }
    Ok(())
}

fn tool(name: &str, root: &Path) -> Command {
    let mut cmd = Command::new("rustup");
    cmd.args(["run", "1.97.1", name]).current_dir(root);
    for key in std::env::vars_os().map(|(k, _)| k) {
        let s = key.to_string_lossy();
        if matches!(
            s.as_ref(),
            "RUSTFLAGS"
                | "RUSTC"
                | "RUSTC_WRAPPER"
                | "RUSTC_WORKSPACE_WRAPPER"
                | "CARGO_ENCODED_RUSTFLAGS"
                | "RUSTC_BOOTSTRAP"
        ) || s.starts_with("CARGO_PROFILE_")
            || s.ends_with("_RUSTFLAGS")
        {
            cmd.env_remove(key);
        }
    }
    cmd
}

fn probe(selection: &Selection) -> Result<String> {
    let api = &selection.api;
    api.validate()?;
    let range = api
        .variants
        .get(&selection.range_error)
        .ok_or("unknown range error")?;
    let mut template = include_str!("probe.rs.txt").to_owned();
    for (key, value) in [
        ("@TYPE@", &api.type_name),
        ("@ERROR@", &api.error_type),
        ("@CONSTRUCTOR@", &api.constructor),
        ("@ACCESSOR@", &api.accessor),
        ("@MAX@", &api.maximum_constant),
        ("@RANGE@", range),
    ] {
        template = template.replace(key, value);
    }
    Ok(template
        .replace("@INTEGER@", &format!("u{}", selection.bits))
        .replace(
            "@VARIANTS@",
            &api.variants
                .values()
                .map(|v| format!("Error::{v}"))
                .collect::<Vec<_>>()
                .join(","),
        ))
}

pub fn recover(manifest: &Path, selection_path: &Path, destination: &Path) -> Result<()> {
    let manifest = manifest.canonicalize().map_err(|e| e.to_string())?;
    if manifest.file_name().and_then(|s| s.to_str()) != Some("Cargo.toml") {
        return Err("expected Cargo.toml".into());
    }
    let input = fs::read(selection_path).map_err(|e| e.to_string())?;
    let s: Selection = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
    if !matches!(s.bits, 8 | 16 | 32 | 64) {
        return Err("invalid unsigned width".into());
    }
    for id in std::iter::once(&s.api.domain).chain(s.api.variants.keys()) {
        NamedNode::new(id.as_str()).map_err(|e| e.to_string())?;
    }
    if s.package.is_empty()
        || !s
            .package
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err("invalid package name".into());
    }
    let program = probe(&s)?;
    let root = manifest.parent().ok_or("missing manifest parent")?;
    clean(root, &s.source_revision)?;
    let metadata: Value = serde_json::from_slice(&output(
        tool("cargo", root)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--offline",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(&manifest),
    )?)
    .map_err(|e| e.to_string())?;
    let workspace = Path::new(
        metadata["workspace_root"]
            .as_str()
            .ok_or("missing workspace root")?,
    );
    let lock = workspace.join("Cargo.lock");
    if digest(&fs::read(&lock).map_err(|e| e.to_string())?) != s.cargo_lock_sha256 {
        return Err("Cargo.lock pin mismatch".into());
    }
    let packages = metadata["packages"]
        .as_array()
        .ok_or("invalid Cargo metadata")?;
    let package = packages
        .iter()
        .find(|p| p["name"] == s.package)
        .ok_or("unknown workspace package")?;
    let compiler =
        String::from_utf8(output(tool("rustc", root).arg("-vV"))?).map_err(|e| e.to_string())?;
    if !compiler
        .lines()
        .any(|l| l.strip_prefix("commit-hash: ") == Some(&s.compiler_commit))
    {
        return Err("compiler pin mismatch".into());
    }
    let host = compiler
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("missing compiler host")?;
    let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
    let target = stage.path().join("target");
    let messages = output(
        tool("cargo", root)
            .args([
                "build",
                "--release",
                "--locked",
                "--offline",
                "--lib",
                "--message-format=json",
                "--target",
                host,
                "--package",
                &s.package,
                "--manifest-path",
            ])
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target),
    )?;
    let mut libs = Vec::new();
    for row in messages.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
        let event: Value = serde_json::from_slice(row).map_err(|e| e.to_string())?;
        if event["reason"] == "compiler-artifact"
            && event["package_id"] == package["id"]
            && event["target"]["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|x| x == "lib" || x == "rlib"))
        {
            libs.extend(
                event["filenames"]
                    .as_array()
                    .ok_or("Cargo artifact filenames missing")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|p| p.ends_with(".rlib"))
                    .map(PathBuf::from),
            );
        }
    }
    let [lib] = libs.as_slice() else {
        return Err("expected one original rlib".into());
    };
    let source = stage.path().join("observe.rs");
    let exe = stage.path().join("observe");
    fs::write(&source, program).map_err(|e| e.to_string())?;
    output(
        tool("rustc", root)
            .arg("--edition=2024")
            .arg(&source)
            .arg("--extern")
            .arg(format!("observed={}", lib.display()))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                target.join(host).join("release/deps").display()
            ))
            .arg("-L")
            .arg(format!(
                "dependency={}",
                target.join("release/deps").display()
            ))
            .arg("-o")
            .arg(&exe),
    )?;
    let data = String::from_utf8(output(
        Command::new(&exe).env_clear().current_dir(stage.path()),
    )?)
    .map_err(|e| e.to_string())?;
    let mut lines = data.lines();
    let bounds = lines
        .next()
        .ok_or("missing observed interval")?
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let [min, max, default] = bounds.as_slice() else {
        return Err("invalid observed interval".into());
    };
    let mut graph = format!(
        "@prefix c: <urn:oddlang:core:> .\n<{}> a c:OptionalUnsignedRange ; c:bits {} ; c:minimum {min} ; c:maximum {max} ; c:defaultValue {default} ; c:rangeError <{}> .\n",
        s.api.domain, s.bits, s.range_error
    );
    for id in s.api.variants.keys() {
        let raw = lines.next().ok_or("missing error display")?;
        if !raw.bytes().all(|b| b.is_ascii_hexdigit()) || raw.len() % 2 != 0 {
            return Err("invalid error display".into());
        }
        let msg = (0..raw.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&raw[i..i + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let literal =
            Literal::new_simple_literal(String::from_utf8(msg).map_err(|e| e.to_string())?);
        graph.push_str(&format!("<{id}> a c:NamedError ; c:message {literal} .\n"));
    }
    if lines.next().is_some() {
        return Err("unexpected observations".into());
    }
    clean(workspace, &s.source_revision)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    fs::write(tmp.path().join("domain.ttl"), graph).map_err(|e| e.to_string())?;
    let domain = super::load(tmp.path())?;
    let receipt = serde_json::json!({"qualification":"candidate","sourceRevision":s.source_revision,"compilerCommit":s.compiler_commit,"cargoLockSha256":s.cargo_lock_sha256,"host":host,"profile":"release","package":s.package,"selectionSha256":digest(&input),"graphSha256":digest(&serde_json::to_vec(&domain).map_err(|e|e.to_string())?),"numericCoverage":if s.bits<=16 {"exhaustive"} else {"sampled; interval property requires independent review"}});
    fs::write(
        tmp.path().join("observation.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if destination.symlink_metadata().is_ok() {
        return Err("destination exists".into());
    }
    fs::rename(tmp.path(), destination).map_err(|e| e.to_string())
}
