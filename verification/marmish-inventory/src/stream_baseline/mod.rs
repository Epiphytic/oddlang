//! Collect stable rustc and Cargo evidence for the pinned marmish-stream package.
use crate::*;
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
mod inputs;
use inputs::{Roots, commands};
pub const LOGICAL: &str = "/oddlang-workspace";
pub const PACKAGE: &str = "marmish-stream";
pub const PACKAGE_ROOT: &str = "crates/marmish-stream/";
pub const RUST_COMMIT: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";
type Vars = BTreeMap<String, String>;
fn hash_file(path: &Path) -> Result<String> {
    let mut sha = Sha256::new();
    io::copy(&mut fs::File::open(path)?, &mut sha)?;
    Ok(format!("{:x}", sha.finalize()))
}
fn sort_json(values: &mut [Value]) {
    values.sort_by_cached_key(Value::to_string);
}
fn sorted(v: &Value) -> Result<Vec<String>> {
    let mut x = strings(v)?;
    x.sort();
    Ok(x)
}
fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    for item in fs::read_dir(dir)? {
        let item = item?;
        let p = item.path();
        if item.file_type()?.is_dir() {
            files.extend(walk(&p)?);
        } else {
            files.push(p);
        }
    }
    files.sort();
    Ok(files)
}
fn span(data: &[u8], path: &str) -> Result<Option<Value>> {
    let re = regex::bytes::Regex::new(r"(?m)^#\[cfg\(test\)\]\r?\nmod tests \{\r?\n")?;
    let mut matches = re.find_iter(data);
    let Some(mark) = matches.next() else {
        return Ok(None);
    };
    ensure!(
        matches.next().is_none(),
        "multiple fixture test modules in {path}"
    );
    let close = regex::bytes::Regex::new(r"(?m)^\}[^\S\r\n]*$")?
        .find_at(data, mark.end())
        .with_context(|| format!("no closing brace for the fixture test module in {path}"))?;
    let end = close.start() + 1;
    Ok(Some(
        json!({"startByte":mark.start(),"endByte":end,"sha256":digest(&data[mark.start()..end])}),
    ))
}
fn tracked(root: &Path, declared: &Map) -> Result<(Map, Map)> {
    let paths: Set = declared
        .keys()
        .filter(|p| p.starts_with(PACKAGE_ROOT) && p.ends_with(".rs"))
        .cloned()
        .collect();
    ensure!(!paths.is_empty(), "no tracked package Rust files");
    let observed: Set = walk(&root.join(PACKAGE_ROOT))?
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .collect();
    ensure!(
        paths == observed,
        "package Rust source set differs: missing {:?}, extra {:?}",
        paths.difference(&observed).collect::<Vec<_>>(),
        observed.difference(&paths).collect::<Vec<_>>()
    );
    let mut sources = Map::new();
    let mut spans = Map::new();
    for p in paths {
        let path = root.join(&p);
        ensure!(
            path.canonicalize()? == path && path.is_file(),
            "missing or invalid Rust source {p}"
        );
        let bytes = fs::read(&path)?;
        let mut blob = format!("blob {}\0", bytes.len()).into_bytes();
        blob.extend(&bytes);
        let id = match text(&declared[&p])?.len() {
            40 => format!("{:x}", Sha1::digest(&blob)),
            64 => digest(&blob),
            _ => bail!("invalid declared Git object width"),
        };
        sources.insert(
            p.clone(),
            json!({"sha256":digest(&bytes),"bytes":bytes.len(),"gitBlob":id}),
        );
        if let Some(s) = span(&bytes, &p)? {
            spans.insert(p, s);
        }
    }
    Ok((sources, spans))
}
fn tool(name: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(name).args(args).output()?;
    ensure!(
        output.status.success(),
        "{name} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
fn git_root(root: &Path) -> Result<Value> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel", "HEAD"])
        .current_dir(root)
        .output()?;
    if !out.status.success() {
        return Ok(Value::Null);
    }
    let text = String::from_utf8(out.stdout)?;
    let mut lines = text.lines();
    let dir = Path::new(lines.next().context("missing git root")?).canonicalize()?;
    if dir == root {
        Ok(json!(lines.next().context("missing git commit")?))
    } else {
        Ok(Value::Null)
    }
}
fn declared(root: &Path) -> Result<Map> {
    let out = Command::new("git")
        .args(["ls-tree", "-rz", "HEAD"])
        .current_dir(root)
        .output()?;
    ensure!(out.status.success(), "git ls-tree failed");
    let mut files = Map::new();
    for line in std::str::from_utf8(&out.stdout)?
        .split('\0')
        .filter(|p| !p.is_empty())
    {
        let (meta, path) = line.split_once('\t').context("invalid ls-tree row")?;
        let mut fields = meta.split_whitespace();
        if fields.nth(1) == Some("blob") {
            files.insert(
                path.into(),
                json!(fields.next().context("missing git blob")?),
            );
        }
    }
    Ok(files)
}
fn run(
    cmd: &[String],
    cwd: &Path,
    vars: &Vars,
    output: &Path,
    stem: &str,
    sandbox: Option<&Path>,
) -> Result<(String, String)> {
    let mut child = if let Some(profile) = sandbox {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-f").arg(profile).args(cmd);
        c
    } else {
        let mut c = Command::new(&cmd[0]);
        c.args(&cmd[1..]);
        c
    };
    let result = child
        .current_dir(cwd)
        .env_clear()
        .envs(vars)
        .stdin(Stdio::null())
        .output()?;
    fs::write(output.join(format!("{stem}.stdout")), &result.stdout)?;
    let err = output.join(format!("{stem}.stderr"));
    fs::write(&err, &result.stderr)?;
    ensure!(
        result.status.success(),
        "{stem} exited {}; see {}",
        result.status,
        err.display()
    );
    Ok((
        String::from_utf8(result.stdout)?,
        String::from_utf8(result.stderr)?,
    ))
}
pub fn list_tests(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.strip_suffix(": test"))
        .map(str::to_owned)
        .collect::<Set>()
        .into_iter()
        .collect()
}
fn outcomes(stdout: &str) -> Result<Map> {
    let re = Regex::new(r"(?m)^test (.+) \.\.\. (ok|FAILED|ignored)(?:,.*)?$")?;
    Ok(re
        .captures_iter(stdout)
        .map(|m| (m[1].into(), json!(&m[2])))
        .collect())
}
fn execute(
    root: &Path,
    vars: &Vars,
    exe: &Path,
    out: &Path,
    stem: &str,
    sandbox: Option<&Path>,
) -> Result<Value> {
    let hash = hash_file(exe)?;
    let call = |flags: &[&str], suffix| {
        let mut cmd = vec![exe.to_string_lossy().into_owned()];
        cmd.extend(flags.iter().map(|v| v.to_string()));
        Ok::<_, anyhow::Error>(run(&cmd, root, vars, out, &format!("{stem}-{suffix}"), sandbox)?.0)
    };
    let names = list_tests(&call(&["--list"], "list")?);
    let ignored = list_tests(&call(&["--list", "--ignored"], "ignored")?);
    ensure!(hash_file(exe)? == hash, "executable changed");
    let results = outcomes(&call(&["--test-threads=1"], "results")?)?;
    ensure!(
        names.iter().eq(results.keys()),
        "incomplete named test outcomes: {stem}"
    );
    ensure!(
        ignored.iter().eq(results
            .iter()
            .filter(|(_, v)| *v == "ignored")
            .map(|(k, _)| k)),
        "ignored test membership changed: {stem}"
    );
    Ok(json!({"executable":exe,"sha256":hash,"listed":names,"ignored":ignored,"outcomes":results}))
}
pub fn compare(original: &Value, candidate: &Value) -> Result<()> {
    ensure!(
        original["role"] == "original" && candidate["role"] == "candidate",
        "receipt roles differ"
    );
    ensure!(
        original["workspaceRoot"] != candidate["workspaceRoot"],
        "candidate is the original workspace"
    );
    for k in [
        "rustc",
        "cargo",
        "target",
        "targetCfg",
        "sourceRemapping",
        "cargoInputs",
        "trackedSources",
        "inlineTestSpans",
    ] {
        ensure!(
            original.get(k).is_some() && original[k] == candidate[k],
            "baseline mismatch: {k}"
        );
    }
    ensure!(
        keys(&original["builds"])? == keys(&candidate["builds"])?,
        "baseline build modes differ"
    );
    for (mode, left) in object(&original["builds"])? {
        let right = &candidate["builds"][mode];
        for k in [
            "rustcInputs",
            "rustdoc",
            "buildScriptFacts",
            "artifactProfiles",
            "docTestNames",
            "docTestOutcomes",
        ] {
            ensure!(
                left.get(k).is_some() && left[k] == right[k],
                "baseline mismatch: {mode}.{k}"
            );
        }
        ensure!(
            keys(&left["suites"])? == keys(&right["suites"])?,
            "test targets differ: {mode}"
        );
        for (suite, reference) in object(&left["suites"])? {
            for k in ["listed", "ignored", "outcomes"] {
                ensure!(
                    reference.get(k).is_some() && reference[k] == right["suites"][suite][k],
                    "named test results differ: {mode}.{suite}"
                );
            }
        }
    }
    Ok(())
}
pub fn capture(
    root: &Path,
    out: &Path,
    original: Option<&Value>,
    home: &Path,
    sandbox: Option<&Path>,
    vendor: Option<&Path>,
) -> Result<Value> {
    let root_s = root.to_str().context("invalid workspace path")?;
    let commit = git_root(root)?;
    let declared = if let Some(old) = original {
        ensure!(
            old["version"] == 2 && old["upstreamCommit"] == PIN && old["role"] == "original",
            "invalid baseline"
        );
        ensure!(
            old["workspaceRoot"] != root_s,
            "candidate is the original workspace"
        );
        object(&old["declaredWorkspacePaths"])?.clone()
    } else {
        ensure!(commit == PIN, "wrong upstream commit");
        ensure!(
            tool(
                "git",
                &[
                    "-C",
                    root_s,
                    "status",
                    "--porcelain",
                    "--untracked-files=all"
                ]
            )?
            .is_empty(),
            "baseline requires a clean upstream checkout"
        );
        declared(root)?
    };
    let (sources, spans) = tracked(root, &declared)?;
    let cargo = PathBuf::from(tool(
        "rustup",
        &["which", "--toolchain", "1.97.1", "cargo"],
    )?)
    .canonicalize()?;
    let tools = cargo.ancestors().nth(2).context("missing toolchain")?;
    let rustc = tools.join("bin/rustc");
    let rustdoc = tools.join("bin/rustdoc");
    let compiler = tool(rustc.to_str().unwrap(), &["-vV"])?;
    ensure!(
        compiler.contains(&format!("commit-hash: {RUST_COMMIT}"))
            && compiler.contains("host: aarch64-apple-darwin"),
        "expected pinned native Apple Silicon rustc"
    );
    let sdk = PathBuf::from(tool("xcrun", &["--show-sdk-path"])?).canonicalize()?;
    let clang = PathBuf::from(tool("xcrun", &["--find", "clang"])?).canonicalize()?;
    let flag = format!("--remap-path-prefix={root_s}={LOGICAL}");
    fs::create_dir(out.join("home"))?;
    fs::create_dir(out.join("tmp"))?;
    let mut vars: Vars = [
        ("PATH", format!("{}/bin:/usr/bin:/bin", tools.display())),
        ("CARGO_NET_OFFLINE", "true".into()),
        ("CARGO_TERM_COLOR", "never".into()),
        ("LC_ALL", "C".into()),
        ("TZ", "UTC".into()),
        ("CARGO_ENCODED_RUSTFLAGS", flag.clone()),
        ("CARGO_ENCODED_RUSTDOCFLAGS", flag),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    for (k, p) in [
        ("HOME", out.join("home")),
        ("CARGO_HOME", home.into()),
        ("TMPDIR", out.join("tmp")),
        ("SDKROOT", sdk.clone()),
        ("CC", clang.clone()),
        ("CXX", clang.with_file_name("clang++")),
        ("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", clang.clone()),
        ("RUSTC", rustc.clone()),
        ("RUSTDOC", rustdoc),
    ] {
        vars.insert(k.into(), p.to_string_lossy().into_owned());
    }
    let cfg: Vec<_> = tool(rustc.to_str().unwrap(), &["--print", "cfg"])?
        .lines()
        .map(str::to_owned)
        .collect::<Set>()
        .into_iter()
        .collect();
    let mut report = json!({
        "version":2,"role":if original.is_some(){"candidate"}else{"original"},"upstreamCommit":PIN,"workspaceRoot":root,"workspaceGitCommit":commit,
        "rustc":compiler,"cargo":tool(cargo.to_str().unwrap(),&["-V"])?,"target":"aarch64-apple-darwin","targetCfg":cfg,"sourceRemapping":LOGICAL,
        "cargoInputs":file_hashes(root,&["Cargo.toml","Cargo.lock","rust-toolchain.toml","crates/marmish-stream/Cargo.toml"])?,
        "declaredWorkspacePaths":declared,"trackedSources":sources,"inlineTestSpans":spans,"builds":{}
    });
    let prefix = [
        "--locked",
        "--offline",
        "-p",
        PACKAGE,
        "--message-format=json",
        "-vv",
    ];
    for (mode, action) in [
        ("test", vec!["test"]),
        ("check", vec!["check", "--all-targets"]),
        ("release", vec!["build", "--release"]),
    ] {
        let target = out.join(format!("{mode}-target"));
        let mut env = vars.clone();
        env.insert(
            "CARGO_TARGET_DIR".into(),
            target.to_string_lossy().into_owned(),
        );
        let mut cmd = vec![cargo.to_string_lossy().into_owned()];
        cmd.extend(action.into_iter().chain(prefix).map(str::to_owned));
        let (stdout, stderr) = run(&cmd, root, &env, out, mode, sandbox)?;
        let messages: Vec<Value> = stdout
            .lines()
            .filter(|l| l.starts_with("{\"reason\":"))
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        ensure!(
            messages
                .iter()
                .any(|m| m["reason"] == "build-finished" && m["success"] == true),
            "no successful Cargo receipt for {mode}"
        );
        let manifest = root.join(PACKAGE_ROOT).join("Cargo.toml");
        let artifacts: Vec<_> = messages
            .iter()
            .filter(|m| {
                m["reason"] == "compiler-artifact"
                    && m["manifest_path"] == manifest.to_str().unwrap()
            })
            .cloned()
            .collect();
        ensure!(
            !artifacts.is_empty(),
            "no compiler artifacts for {PACKAGE} in {mode}"
        );
        let roots = Roots {
            source: root,
            target: &target,
            home,
            vendor,
            tools,
            sdk: &sdk,
            declared: &declared,
        };
        let commands = commands(&stderr)?;
        let (producers, facts, hashes) = roots.producers(&messages, &commands, &env)?;
        let (records, documents) = roots.inventory(&commands, &producers)?;
        let mut observed = Set::new();
        for record in &records {
            for item in array(&record["inputs"])? {
                if item[0] == "workspace" {
                    observed.insert(text(&item[1])?.to_owned());
                }
            }
        }
        ensure!(
            sources.keys().all(|p| observed.contains(p)),
            "tracked Rust paths missing from {mode} dep-info"
        );
        let mut profiles = vec![];
        for artifact in &artifacts {
            let mut t = artifact["target"].clone();
            t.as_object_mut()
                .context("missing rustc target")?
                .remove("src_path");
            profiles.push(
                json!({"target":t,"profile":artifact["profile"],"features":artifact["features"]}),
            );
        }
        sort_json(&mut profiles);
        let mut build = json!({"command":cmd,"artifacts":artifacts,"rustcInputs":records,"rustdoc":documents,"buildScriptFacts":facts,
            "buildScriptRunHashes":hashes,"artifactProfiles":profiles,"docTestNames":[],"docTestOutcomes":{},"suites":{}});
        if mode == "test" {
            let mut unit_names = Set::new();
            for a in &artifacts {
                if a["executable"].is_null() {
                    continue;
                }
                let name = text(&a["target"]["name"])?;
                let exe = Path::new(text(&a["executable"])?);
                ensure!(
                    exe.canonicalize()?.starts_with(&target),
                    "test executable escaped target"
                );
                ensure!(
                    build["suites"].get(name).is_none(),
                    "duplicate test target {name}"
                );
                let actual: Vec<_> = commands
                    .iter()
                    .filter(|c| c.argv.as_slice() == [exe.to_string_lossy()])
                    .collect();
                ensure!(
                    actual.len() == 1,
                    "no unique Cargo execution for {}",
                    exe.display()
                );
                let mut supplied = env.clone();
                supplied.extend(actual[0].env.clone());
                let suite = execute(
                    manifest.parent().unwrap(),
                    &supplied,
                    exe,
                    out,
                    &format!("test-{name}"),
                    sandbox,
                )?;
                unit_names.extend(keys(&suite["outcomes"])?);
                build["suites"][name] = suite;
            }
            ensure!(
                !object(&build["suites"])?.is_empty(),
                "no test suite observed for {PACKAGE}"
            );
            if !documents.is_empty() {
                let mut doc = vec![cargo.to_string_lossy().into_owned(), "test".into()];
                doc.extend(
                    prefix
                        .into_iter()
                        .chain(["--doc", "--", "--list"])
                        .map(str::to_owned),
                );
                let names = list_tests(&run(&doc, root, &env, out, "doctest-list", sandbox)?.0);
                let mut results = outcomes(&stdout)?;
                results.retain(|k, _| !unit_names.contains(k));
                ensure!(
                    names.iter().eq(results.keys()),
                    "incomplete named doctest outcomes"
                );
                build["docTestNames"] = json!(names);
                build["docTestOutcomes"] = json!(results);
            }
        }
        report["builds"][mode] = build;
    }
    Ok(report)
}
#[cfg(test)]
mod tests;
