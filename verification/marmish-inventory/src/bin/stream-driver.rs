//! Observe local codec and state transitions using the pinned public Rust driver.
use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use marmish_inventory::{
    Map, array, digest, object, read_json, root,
    stream_observer::{expand, matches, peer_cases},
    text, valid_hash,
};
use serde::{Serialize, ser::SerializeMap};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
#[path = "stream_driver/identity.rs"]
mod identity;
use identity::{Workspace, binding, identify};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    workspace: PathBuf,
    #[arg(long)]
    target_dir: PathBuf,
    #[arg(long)]
    receipt: PathBuf,
    #[arg(long,value_parser=["original","candidate","mutation"])]
    role: String,
    #[arg(long)]
    peer_receipt: Option<PathBuf>,
    #[arg(long)]
    sandbox: Option<PathBuf>,
    #[arg(long)]
    toolchain: Option<PathBuf>,
    #[arg(long)]
    baseline: PathBuf,
    #[arg(long)]
    commit: Option<String>,
    #[arg(long)]
    archive_sha256: Option<String>,
    #[arg(long)]
    bundle_sha256: Option<String>,
}
fn sha(p: &Path) -> Result<String> {
    let mut h = Sha256::new();
    io::copy(&mut fs::File::open(p)?, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}
fn path(p: &Path) -> Result<String> {
    Ok(p.to_str().context("path must be UTF-8")?.to_string())
}
fn command(prefix: &[String], args: &[String]) -> Command {
    let mut cmd = prefix.iter().chain(args);
    let mut child = Command::new(cmd.next().expect("missing command"));
    child.args(cmd);
    child
}
fn tool(name: &str, args: &Args) -> Result<Vec<String>> {
    Ok(if let Some(p) = &args.toolchain {
        vec![path(&p.join("bin").join(name))?]
    } else {
        vec![name.to_string(), "+1.97.1".into()]
    })
}
fn log_command(args: &[String]) -> String {
    args.iter()
        .map(|s| {
            if !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
            {
                s.clone()
            } else {
                format!("'{}'", s.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
#[derive(Serialize, Clone)]
struct Artifact {
    path: PathBuf,
    sha256: String,
}
#[derive(Default)]
struct Libraries(Vec<(String, Artifact)>);
impl Serialize for Libraries {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, item) in &self.0 {
            map.serialize_entry(name, item)?;
        }
        map.end()
    }
}
fn build(
    workspace: &Path,
    target: &Path,
    mut cargo: Vec<String>,
    prefix: &[String],
) -> Result<(Libraries, Vec<String>)> {
    cargo.extend(
        [
            "build",
            "--locked",
            "--offline",
            "-p",
            "marmish-stream",
            "--manifest-path",
        ]
        .map(str::to_owned),
    );
    cargo.push(path(&workspace.join("Cargo.toml"))?);
    cargo.extend([
        "--target-dir".into(),
        path(target)?,
        "--message-format=json-render-diagnostics".into(),
    ]);
    let output = command(prefix, &cargo)
        .current_dir(workspace)
        .stderr(Stdio::inherit())
        .output()?;
    ensure!(output.status.success(), "Cargo build failed");
    let mut libs = Libraries::default();
    let mut names = BTreeSet::new();
    for line in output
        .stdout
        .split(|&b| b == b'\n')
        .filter(|s| !s.is_empty())
    {
        let row: Value = serde_json::from_slice(line)?;
        if row["reason"] != "compiler-artifact" {
            continue;
        }
        let name = text(&row["target"]["name"])?;
        if !matches!(name, "marmish_stream" | "serde_json") {
            continue;
        }
        let rlibs: Vec<_> = array(&row["filenames"])?
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|p| p.ends_with(".rlib"))
            .collect();
        ensure!(
            rlibs.len() == 1 && row["fresh"] == false && names.insert(name.to_owned()),
            "unexpected cached or ambiguous artifact for {name}"
        );
        let lib = PathBuf::from(rlibs[0]);
        let real = lib.canonicalize()?;
        let parent = real.parent().context("invalid native artifact")?;
        ensure!(
            parent == target.join("debug/deps") || parent == target.join("debug"),
            "Cargo artifact outside the selected native target"
        );
        libs.0.push((
            name.into(),
            Artifact {
                sha256: sha(&lib)?,
                path: lib,
            },
        ));
    }
    ensure!(
        libs.0.len() == 2,
        "build did not report both downstream driver libraries"
    );
    Ok((libs, cargo))
}
fn compile(
    target: &Path,
    source: &Path,
    libs: &Libraries,
    mut rustc: Vec<String>,
    prefix: &[String],
    role: &str,
    attachment: &str,
) -> Result<(PathBuf, Vec<String>)> {
    let executable = target.join("stream-oracle");
    rustc.extend([
        "--edition=2024".into(),
        "--crate-name".into(),
        "stream_oracle".into(),
        path(source)?,
        "-L".into(),
        format!("dependency={}", target.join("debug/deps").display()),
        "-o".into(),
        path(&executable)?,
    ]);
    for (name, artifact) in libs
        .0
        .iter()
        .map(|(name, lib)| (name, lib))
        .collect::<BTreeMap<_, _>>()
    {
        rustc.extend([
            "--extern".into(),
            format!("{name}={}", artifact.path.display()),
        ]);
    }
    ensure!(
        command(prefix, &rustc)
            .env("ODDLANG_STREAM_DRIVER_ROLE", role)
            .env("ODDLANG_STREAM_DRIVER_BINDING", attachment)
            .status()?
            .success(),
        "driver compilation failed"
    );
    Ok((executable, rustc))
}
#[derive(Serialize)]
struct Observation {
    actual: Value,
    name: String,
    matched: bool,
}
fn run_cases(
    exe: &Path,
    role: &str,
    attachment: &str,
    cases: &[Value],
    prefix: &[String],
    cwd: &Path,
) -> Result<(String, Vec<Observation>)> {
    let pin = sha(exe)?;
    let mut child = command(prefix, &[path(exe)?])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    serde_json::to_writer(child.stdin.take().unwrap(), cases)?;
    let output = child.wait_with_output()?;
    ensure!(output.status.success(), "stream observation failed");
    let result: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(result["role"] == role, "unexpected driver role");
    ensure!(
        result["binding"] == attachment,
        "driver executable is not bound to the selected workspace and artifacts"
    );
    ensure!(
        sha(exe)? == pin,
        "driver executable changed during execution"
    );
    let rows = array(&result["cases"])?;
    ensure!(
        rows.len() == cases.len(),
        "missing or unexpected driver cases"
    );
    let mut observed = vec![];
    for (case, row) in cases.iter().zip(rows) {
        ensure!(
            row["name"] == case["name"],
            "missing or unexpected driver cases"
        );
        observed.push(Observation {
            actual: row["actual"].clone(),
            name: text(&row["name"])?.into(),
            matched: matches(&row["actual"], &expand(&case["expected"])?),
        });
    }
    Ok((pin, observed))
}
#[derive(Serialize)]
struct Receipt {
    schema: u8,
    role: String,
    workspace: PathBuf,
    target: PathBuf,
    upstream_commit: String,
    lock_sha256: String,
    vectors_sha256: String,
    driver_source_sha256: String,
    driver_sha256: String,
    driver_path: PathBuf,
    driver_binding_sha256: String,
    workspace_identity: Workspace,
    cargo_command: String,
    rustc_command: String,
    rustc_version: String,
    libraries: Libraries,
    observations: Vec<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_receipt_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mixed_observations: Option<Vec<Observation>>,
    all_matched: bool,
}
fn output_path(input: &Path) -> Result<PathBuf> {
    if input.exists() {
        return Ok(input.canonicalize()?);
    }
    let name = input.file_name().context("invalid target directory")?;
    let parent = input.parent().context("invalid target parent")?;
    Ok(output_path(parent)?.join(name))
}
fn main() -> Result<()> {
    let args = Args::parse();
    let workspace = args.workspace.canonicalize()?;
    let target = output_path(&std::path::absolute(&args.target_dir)?)?;
    let prefix = if let Some(sandbox) = &args.sandbox {
        vec![
            "/usr/bin/sandbox-exec".to_owned(),
            "-f".into(),
            path(&sandbox.canonicalize()?)?,
        ]
    } else {
        vec![]
    };
    let rustc = tool("rustc", &args)?;
    let mut version_cmd = rustc.clone();
    version_cmd.push("-vV".into());
    let result = command(&prefix, &version_cmd).output()?;
    let version = String::from_utf8(result.stdout)?.trim().to_owned();
    ensure!(
        result.status.success() && version.starts_with("rustc 1.97.1 "),
        "expected the pinned Rust 1.97.1 compiler"
    );
    ensure!(
        !target.exists() || fs::read_dir(&target)?.next().is_none(),
        "target directory must be empty to rule out previously built artifacts"
    );
    let base = root().join("examples/marmish");
    let source = base.join("stream-driver/main.rs");
    let vectors = base.join("stream-vectors.json");
    let vector_pin = sha(&vectors)?;
    let source_pin = sha(&source)?;
    let corpus = read_json(&vectors)?;
    let cases = array(&corpus["cases"])?;
    let names: BTreeSet<_> = cases
        .iter()
        .map(|c| text(&c["name"]))
        .collect::<Result<_>>()?;
    ensure!(
        corpus["schema"] == 1 && names.len() == cases.len(),
        "invalid frozen vectors"
    );
    let upstream = text(&corpus["upstream_commit"])?;
    let provenance = identify(&args, &workspace, upstream)?;
    let (libs, cargo_cmd) = build(&workspace, &target, tool("cargo", &args)?, &prefix)?;
    let attachment = binding(&json!({"workspace":provenance,"libraries":libs}));
    let (exe, rustc_cmd) = compile(
        &target,
        &source,
        &libs,
        rustc,
        &prefix,
        &args.role,
        &attachment,
    )?;
    ensure!(
        json!(identify(&args, &workspace, upstream)?) == json!(provenance),
        "workspace changed before driver execution"
    );
    for (_, artifact) in &libs.0 {
        ensure!(
            sha(&artifact.path)? == artifact.sha256,
            "linked library changed before driver execution"
        );
    }
    let (pin, observations) = run_cases(&exe, &args.role, &attachment, cases, &prefix, &workspace)?;

    let (mut mixed, mut peer_pin) = (None, None);
    if let Some(peer_path) = &args.peer_receipt {
        let raw = fs::read(peer_path)?;
        let peer: Value = serde_json::from_slice(&raw)?;
        ensure!(
            peer["driver_sha256"] != pin,
            "peer and local driver binaries have identical identities"
        );
        let replay = peer_cases(
            &peer,
            cases,
            &vector_pin,
            &source_pin,
            &json!(provenance),
            binding,
        )?;
        let (replayed, rows) =
            run_cases(&exe, &args.role, &attachment, &replay, &prefix, &workspace)?;
        ensure!(
            replayed == pin,
            "mixed wire replay used a different executable"
        );
        mixed = Some(rows);
        peer_pin = Some(digest(&raw));
    }
    let failures: Vec<_> = observations
        .iter()
        .chain(mixed.iter().flatten())
        .filter(|r| !r.matched)
        .map(|r| r.name.clone())
        .collect();
    let role = args.role.clone();
    let count = observations.len();
    let wire = mixed.as_ref().map(Vec::len).unwrap_or(0);
    let receipt = Receipt {
        schema: 1,
        role: args.role,
        lock_sha256: sha(&workspace.join("Cargo.lock"))?,
        workspace,
        target,
        upstream_commit: upstream.into(),
        vectors_sha256: vector_pin,
        driver_source_sha256: source_pin,
        driver_sha256: pin.clone(),
        driver_path: exe,
        driver_binding_sha256: attachment,
        workspace_identity: provenance,
        cargo_command: log_command(&cargo_cmd),
        rustc_command: log_command(&rustc_cmd),
        rustc_version: version,
        libraries: libs,
        observations,
        peer_receipt_sha256: peer_pin,
        mixed_observations: mixed,
        all_matched: failures.is_empty(),
    };
    if let Some(dir) = args.receipt.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut out = fs::File::create(&args.receipt)?;
    serde_json::to_writer_pretty(&mut out, &receipt)?;
    writeln!(out)?;
    if !failures.is_empty() {
        bail!("Failed vectors: {}", failures.join(", "));
    }
    println!("{role}: {count} vectors, {wire} mixed observations, binary {pin}");
    Ok(())
}
#[cfg(test)]
#[path = "stream_driver/tests.rs"]
mod tests;
