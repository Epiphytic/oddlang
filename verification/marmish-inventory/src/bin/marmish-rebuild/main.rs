//! Recover, independently assemble and index the two-producer Marmish candidate.
#[path = "../phase7/reconstruction.rs"]
mod reconstruction;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use marmish_inventory::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
mod assemble;
mod candidate;
#[derive(Parser)]
struct Args {
    upstream: PathBuf,
    output: PathBuf,
    /// Use an already built Oddlang executable for recovery and projection.
    #[arg(long, conflicts_with = "replay")]
    oddlang: Option<PathBuf>,
    /// Validate the archived recovery, immutable Git objects, and independent builds.
    #[arg(long)]
    replay: bool,
}
fn output(c: &mut Command) -> Result<Vec<u8>> {
    let o = c
        .env("CARGO_NET_OFFLINE", "true")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .with_context(|| format!("start {c:?}"))?;
    ensure!(
        o.status.success(),
        "{:?} exited {}: {}",
        c.get_program(),
        o.status,
        String::from_utf8_lossy(&o.stderr)
    );
    Ok(o.stdout)
}
fn line(c: &mut Command) -> Result<String> {
    Ok(String::from_utf8(output(c)?)?.trim().to_owned())
}
fn git(path: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
        ])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    c
}
fn source_inventory(upstream: &Path) -> Result<Value> {
    ensure!(
        line(git(upstream).args(["rev-parse", "HEAD"]))? == PIN,
        "unexpected original revision"
    );
    ensure!(
        output(git(upstream).args(["status", "--porcelain", "--untracked-files=all"]))?.is_empty(),
        "original checkout has modifications"
    );
    let bytes = output(git(upstream).args(["ls-tree", "-r", "-z", PIN]))?;
    let mut files = Map::new();
    for row in std::str::from_utf8(&bytes)?.split_terminator('\0') {
        let (header, path) = row.split_once('\t').context("missing tree entry path")?;
        let fields: Vec<_> = header.split_whitespace().collect();
        ensure!(
            fields.len() == 3 && fields[1] == "blob",
            "unexpected original tree entry"
        );
        let bytes = read(&upstream.join(path))?;
        ensure!(
            reconstruction::git_object("blob", &bytes) == fields[2],
            "original blob differs: {path}"
        );
        files.insert(
            path.into(),
            json!({"mode":fields[0],"gitBlob":fields[2],"sha256":digest(&bytes)}),
        );
    }
    Ok(json!({"files":files}))
}
fn write_json(path: &Path, v: &Value) -> Result<()> {
    let mut v = v.clone();
    v.sort_all_objects();
    fs::write(path, serde_json::to_string_pretty(&v)? + "\n")?;
    Ok(())
}
fn equal_outputs(out: &Path) -> Result<()> {
    for name in [
        "framing",
        "facade",
        "framing-projection",
        "facade-projection",
        "immutable",
    ] {
        let a = out.join("first").join(name);
        let b = out.join("second").join(name);
        let names = filenames(&a)?;
        ensure!(
            names == filenames(&b)?,
            "independent membership differs: {name}"
        );
        for file in names {
            ensure!(
                read(&a.join(&file))? == read(&b.join(&file))?,
                "nonrepeatable recovery or projection: {name}/{file}"
            );
        }
    }
    Ok(())
}
fn compiler(root: &Path) -> Result<PathBuf> {
    let bytes = output(
        Command::new("cargo")
            .args([
                "+1.97.1",
                "build",
                "--locked",
                "--offline",
                "--bin",
                "oddlang",
                "--message-format=json-render-diagnostics",
            ])
            .current_dir(root),
    )?;
    for line in bytes.split(|&b| b == b'\n').filter(|b| !b.is_empty()) {
        let m: Value = serde_json::from_slice(line)?;
        if m["reason"] == "compiler-artifact"
            && m["target"]["name"] == "oddlang"
            && let Some(exe) = m["executable"].as_str()
        {
            return Ok(PathBuf::from(exe));
        }
    }
    anyhow::bail!("Cargo did not report an Oddlang executable")
}
fn main() -> Result<()> {
    let a = Args::parse();
    let upstream = a.upstream.canonicalize()?;
    let inventory = source_inventory(&upstream)?;
    let root = root();
    let releases = root.join("examples/marmish");
    let out = a.output;
    if !a.replay {
        fs::create_dir_all(&out)?;
        ensure!(
            fs::read_dir(&out)?.next().is_none(),
            "expected an empty output directory"
        );
    }
    let out = out.canonicalize()?;
    let summary = if !a.replay {
        let binary = if let Some(p) = a.oddlang {
            p.canonicalize()?
        } else {
            compiler(&root)?
        };
        let binary_hash = digest(&read(&binary)?);
        let sandbox = assemble::Recovery {
            upstream: &upstream,
            root: &root,
            binary: &binary,
        };
        let run = |build: &str, dest: &Path| -> Result<Value> {
            let evidence = out.join(build);
            fs::create_dir(&evidence)?;
            sandbox.assemble(dest, &evidence)?;
            candidate::index(&upstream, dest, &evidence, &inventory, &releases)
        };
        let first = run("first", &out.join("candidate"))?;
        let other = tempfile::Builder::new()
            .prefix("oddlang-independent-rebuild-")
            .tempdir()?;
        let second = run("second", &other.path().join("rebuilt-elsewhere"))?;
        ensure!(
            first == second
                && binary_hash == digest(&read(&binary)?)
                && inventory == source_inventory(&upstream)?,
            "independent rebuild inputs or results changed"
        );
        equal_outputs(&out)?;
        let summary = reconstruction::verified_summary(&out, &inventory, &releases)?;
        write_json(&out.join("rebuild.json"), &summary)?;
        summary
    } else {
        equal_outputs(&out)?;
        reconstruction::build_pair(&out, &inventory, &releases)?
    };
    println!(
        "Two builds verified for {} ({} source files, {} retained)",
        text(&summary["candidateCommit"])?,
        summary["sourceFiles"],
        summary["retainedFiles"]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_recovery_outputs_require_equal_membership_and_bytes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for build in ["first", "second"] {
            for family in [
                "framing",
                "facade",
                "framing-projection",
                "facade-projection",
                "immutable",
            ] {
                let dir = temp.path().join(build).join(family);
                fs::create_dir_all(&dir)?;
                fs::write(dir.join("witness"), b"original")?;
            }
        }
        equal_outputs(temp.path())?;
        let right = temp.path().join("second/framing/witness");
        fs::write(&right, b"altered")?;
        assert!(
            equal_outputs(temp.path())
                .unwrap_err()
                .to_string()
                .contains("nonrepeatable")
        );
        fs::remove_file(right)?;
        assert!(
            equal_outputs(temp.path())
                .unwrap_err()
                .to_string()
                .contains("membership")
        );
        Ok(())
    }
}
