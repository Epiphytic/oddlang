use super::corpus::{directory, hash};
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use marmish_inventory::{PIN, array, digest, text};
use serde_json::{Value as V, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};
#[derive(Args)]
pub struct Original {
    #[arg(long,default_value=PIN)]
    pub original_revision: String,
    #[arg(
        long,
        default_value = "9c5f7dde5c0f7d340e22dbefaa8a1c6ddb94c119011ba3e49ac8044c9b4cb0b3"
    )]
    pub original_lock: String,
    #[arg(long,default_value="dev",value_parser=["dev","release"])]
    pub original_profile: String,
    #[arg(long,default_value="aarch64-apple-darwin",value_parser=["aarch64-apple-darwin","aarch64-unknown-linux-gnu","x86_64-unknown-linux-gnu"])]
    pub original_target: String,
}
pub fn sum(path: &Path) -> Result<String> {
    Ok(digest(&fs::read(path)?))
}
pub fn run(cmd: &mut Command, stdin: &[u8], secs: u64) -> Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    let mut input = tempfile::tempfile()?;
    input.write_all(stdin)?;
    input.rewind()?;
    let (mut out, mut err) = (tempfile::tempfile()?, tempfile::tempfile()?);
    let mut child = cmd
        .stdin(input)
        .stdout(out.try_clone()?)
        .stderr(err.try_clone()?)
        .spawn()
        .with_context(|| format!("start {:?}", cmd.get_program()))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(secs) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("command timed out after {secs}s");
        }
        thread::sleep(Duration::from_millis(15));
    };
    fn bytes(file: &mut fs::File) -> Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = vec![];
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    Ok((status, bytes(&mut out)?, bytes(&mut err)?))
}
pub fn checked(cmd: &mut Command, seconds: u64) -> Result<String> {
    let (s, o, e) = run(cmd, &[], seconds)?;
    ensure!(
        s.success(),
        "{:?} failed: {}",
        cmd.get_program(),
        String::from_utf8_lossy(&e)
    );
    Ok(String::from_utf8(o)?)
}
pub fn command(args: &[String]) -> Result<Command> {
    let (prog, rest) = args.split_first().context("empty command")?;
    let mut c = Command::new(prog);
    c.args(rest);
    Ok(c)
}
fn pin(work: &Path, args: &Original) -> Result<V> {
    let git = |a: &[&str]| checked(Command::new("git").current_dir(work).args(a), 20);
    let rev = git(&["rev-parse", "HEAD"])?;
    ensure!(
        rev.trim() == args.original_revision,
        "workspace revision differs"
    );
    ensure!(
        git(&["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
        "workspace contains modifications"
    );
    let lock = sum(&work.join("Cargo.lock"))?;
    ensure!(lock == args.original_lock, "unexpected Cargo.lock digest");
    Ok(json!({"revision":rev.trim(),"cargoLockSha256":lock}))
}
fn clean_build(cmd: &mut Command) -> &mut Command {
    cmd.env_clear();
    for k in ["HOME", "PATH", "CARGO_HOME", "RUSTUP_HOME"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    cmd.env("LANG", "C")
        .env("LC_ALL", "C")
        .env("CARGO_NET_OFFLINE", "true")
}
pub fn build(work: &Path, target: &Path, args: &Original) -> Result<(Vec<String>, V)> {
    let mut provenance = pin(work, args)?;
    let compiler = checked(Command::new("rustc").args(["+1.97.1", "-vV"]), 20)?;
    let triple = &args.original_target;
    ensure!(
        compiler.starts_with("rustc 1.97.1 ")
            && compiler.lines().any(|l| l == format!("host: {triple}")),
        "unexpected rustc or target"
    );
    fs::create_dir_all(target)?;
    let target = target.canonicalize()?;
    let profile = &args.original_profile;
    let mut cargo = Command::new("cargo");
    clean_build(&mut cargo)
        .current_dir(work)
        .args([
            "+1.97.1",
            "build",
            "--locked",
            "--offline",
            "--profile",
            profile,
            "--target",
            triple,
            "--package",
            "marmish-stream",
            "--target-dir",
        ])
        .arg(&target)
        .arg("--message-format=json-render-diagnostics");
    let mut libs = BTreeMap::new();
    let mut features = BTreeMap::new();
    for line in checked(&mut cargo, 180)?.lines() {
        let m: V = serde_json::from_str(line)?;
        if m["reason"] != "compiler-artifact" {
            continue;
        }
        let name = text(&m["target"]["name"])?;
        if ["marmish_stream", "serde", "serde_json", "flate2"].contains(&name) {
            ensure!(
                features
                    .insert(name.to_string(), m["features"].clone())
                    .is_none(),
                "ambiguous library {name}"
            );
            if name != "flate2" {
                let files: Vec<_> = array(&m["filenames"])?
                    .iter()
                    .filter_map(V::as_str)
                    .filter(|s| s.ends_with(".rlib"))
                    .collect();
                ensure!(
                    files.len() == 1 && m["fresh"] == false,
                    "expected one freshly linked {name}"
                );
                let path = PathBuf::from(files[0]).canonicalize()?;
                ensure!(path.starts_with(&target), "unexpected build path");
                libs.insert(name.to_string(), path);
            }
        }
    }
    ensure!(libs.len() == 3 && features.len() == 4, "missing libraries");
    let hashes = |libs: &BTreeMap<String, PathBuf>| {
        libs.iter()
            .map(|(n, p)| Ok((n.clone(), sum(p)?)))
            .collect::<Result<BTreeMap<_, _>>>()
    };
    let linked = hashes(&libs)?;
    let source = directory().join("stream.rs");
    let sources = json!({"stream.rs":sum(&source)?,"archived-main.rs":sum(&directory().join("../stream-driver/main.rs"))?});
    let binding =
        hash(&json!({"upstream":provenance,"compiler":compiler,"sources":sources,"libs":linked}))?;
    let caller = target.join("stream-oracle");
    let profile_dir = if profile == "dev" { "debug" } else { "release" };
    let mut rustc = Command::new("rustc");
    clean_build(&mut rustc)
        .current_dir(work)
        .env("ODDLANG_STREAM_DRIVER_BINDING", &binding)
        .env("ODDLANG_STREAM_DRIVER_ROLE", "original")
        .args([
            "+1.97.1",
            "--edition",
            "2024",
            "--crate-name",
            "stream_oracle",
            "--target",
            triple,
        ])
        .arg(&source);
    for d in [target.join(triple), target.clone()] {
        rustc.arg("-L").arg(format!(
            "dependency={}",
            d.join(profile_dir).join("deps").display()
        ));
    }
    rustc.arg("-o").arg(&caller);
    for (n, p) in &libs {
        rustc.arg("--extern").arg(format!("{n}={}", p.display()));
    }
    checked(&mut rustc, 45)?;
    ensure!(
        pin(work, args)? == provenance && linked == hashes(&libs)?,
        "build identity changed"
    );
    let argv = |c: &Command| {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|arg| {
                let s = arg.to_string_lossy();
                if !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
                {
                    s.into_owned()
                } else {
                    format!("'{}'", s.replace('\'', "'\"'\"'"))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    provenance.as_object_mut().unwrap().extend(
        json!({
            "features":features,
            "profile":profile,
            "target":triple,
            "compiler":compiler.lines().collect::<Vec<_>>(),
            "callerBinding":binding,
            "callerSha256":sum(&caller)?,
            "librarySha256":linked,
            "sourceSha256":sources,
            "cargoCommand":argv(&cargo),
            "rustcCommand":argv(&rustc),
            "buildLocale":"C",
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    Ok((vec![caller.display().to_string()], provenance))
}
pub fn observe(args: &[String], cases: &[V], binding: Option<&str>) -> Result<Vec<V>> {
    let rows: Vec<_> = cases
        .iter()
        .map(|r| json!({"name":r["name"],"operation":r["operation"],"input":r["input"]}))
        .collect();
    let dir = tempfile::tempdir()?;
    let mut c = command(args)?;
    c.current_dir(dir.path()).env_clear().envs([
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("TZ", "UTC"),
    ]);
    let (s, o, e) = run(&mut c, &serde_json::to_vec(&rows)?, 30)?;
    ensure!(
        s.success() && e.is_empty(),
        "adapter failed or wrote stderr"
    );
    let response: V = serde_json::from_slice(&o)?;
    if let Some(b) = binding {
        ensure!(response["binding"] == b, "adapter binding mismatch");
    }
    let reports = array(&response["cases"])?;
    ensure!(
        reports.len() == cases.len()
            && reports
                .iter()
                .zip(cases)
                .all(|(r, c)| r["name"] == c["name"] && r.get("actual").is_some()),
        "missing or reordered observations"
    );
    Ok(reports.clone())
}
pub fn holdout_boundary(path: &Path) -> Result<Vec<String>> {
    ensure!(
        cfg!(target_os = "macos"),
        "sealed original replay requires the macOS qualification boundary"
    );
    let path = path.canonicalize()?;
    let parent = serde_json::to_string(path.parent().context("holdout directory")?)?;
    let prefix = vec![
        "/usr/bin/sandbox-exec".into(),
        "-p".into(),
        format!("(version 1)(allow default)(deny file-read* file-write* (subpath {parent}))"),
    ];
    let mut check = command(&prefix)?;
    check.arg("/bin/cat").arg(&path);
    let (s, o, e) = run(&mut check, &[], 10)?;
    ensure!(
        s.code() == Some(1)
            && o.is_empty()
            && String::from_utf8_lossy(&e).contains("Operation not permitted"),
        "direct holdout read probe failed"
    );
    Ok(prefix)
}
