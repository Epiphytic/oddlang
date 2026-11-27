//! Compile the unchanged native core caller and compare both directions locally.
use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
#[path = "../framing_contract.rs"]
mod framing_contract;
use framing_contract::{cases, expand, hash, hex};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
const REV: &str = "49fc705408a230bd0ad896248d08dbdd2faad05a";
const CODEC: &str = "72b3254f16251a8381aa12e40e3c4d2f0199f8c6508fbecb9d91f575e0fbb8c6";
const CONTRACT: &str = include_str!("../../../../examples/marmish/observations.json");
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    Collect {
        upstream: PathBuf,
        empty_output: PathBuf,
    },
    Compare {
        upstream: PathBuf,
        candidate: PathBuf,
        output: PathBuf,
    },
}
fn run(cmd: &mut Command) -> Result<Vec<u8>> {
    let result = cmd.output()?;
    ensure!(
        result.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(result.stdout)
}
fn source(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/marmish")
        .join(name)
}
fn checked_text(cmd: &mut Command) -> Result<String> {
    Ok(String::from_utf8(run(cmd)?)?.trim().to_owned())
}
fn save(path: &Path, v: &Value) -> Result<()> {
    let mut f = fs::File::create(path)?;
    serde_json::to_writer_pretty(&mut f, v)?;
    f.write_all(b"\n")?;
    Ok(())
}
struct Consumer {
    exe: PathBuf,
    compiler: String,
    host: String,
    base64: PathBuf,
}
fn library(artifacts: &[Value], name: &str, package: Option<&str>) -> Result<PathBuf> {
    let rows: Vec<_> = artifacts
        .iter()
        .filter(|a| {
            a["reason"] == "compiler-artifact"
                && a["target"]["name"] == name
                && package.is_none_or(|suffix| {
                    a["package_id"]
                        .as_str()
                        .is_some_and(|p| p.ends_with(suffix))
                })
        })
        .collect();
    ensure!(rows.len() == 1, "expected one {name} Cargo artifact");
    let libs: Vec<_> = rows[0]["filenames"]
        .as_array()
        .context("missing filenames")?
        .iter()
        .filter_map(Value::as_str)
        .filter(|p| p.ends_with(".rlib"))
        .collect();
    ensure!(libs.len() == 1, "expected a single {name} library");
    Ok(libs[0].into())
}
fn build(repo: &Path, stage: &Path) -> Result<Consumer> {
    fs::create_dir_all(stage)?;
    let compiler = checked_text(Command::new("rustc").args(["+1.97.1", "-Vv"]))?;
    let host = compiler
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .context("missing host triple")?
        .to_owned();
    let target = stage.join("target");
    let raw = run(Command::new("cargo")
        .current_dir(repo)
        .env("CARGO_TARGET_DIR", &target)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "+1.97.1",
            "build",
            "--locked",
            "--offline",
            "-p",
            "marmish-core",
            "--lib",
            "--message-format=json",
            "--target",
            &host,
        ]))?;
    let artifacts = String::from_utf8(raw)?
        .lines()
        .map(serde_json::from_str)
        .collect::<serde_json::Result<Vec<Value>>>()?;
    let core = library(&artifacts, "marmish_core", None)?;
    let base64 = library(&artifacts, "base64", Some("#base64@0.22.1"))?;
    let exe = stage.join("consumer");
    run(Command::new("rustc")
        .current_dir(repo)
        .args(["+1.97.1", "--edition=2024", "--target", &host, "--extern"])
        .arg(format!("marmish_core={}", core.display()))
        .arg("-L")
        .arg(format!(
            "dependency={}",
            target.join(&host).join("debug/deps").display()
        ))
        .arg("-L")
        .arg(format!(
            "dependency={}",
            target.join("debug/deps").display()
        ))
        .arg(source("observe_core.rs"))
        .arg("-o")
        .arg(&exe))?;
    run(Command::new(&exe).arg("api"))?;
    Ok(Consumer {
        exe,
        compiler,
        host,
        base64,
    })
}
fn exchange(exe: &Path, operation: &str, input: &[u8]) -> Result<Output> {
    ensure!(
        matches!(operation, "encode" | "decode"),
        "unknown framing operation"
    );
    let mut child = Command::new(exe)
        .arg(operation)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(input)?;
    Ok(child.wait_with_output()?)
}
fn observe_output(result: Output) -> Result<Value> {
    if result.status.success() && result.stderr.is_empty() {
        Ok(json!({"bytesHex":hex(&result.stdout)}))
    } else if result.status.code() == Some(2)
        && result.stdout.is_empty()
        && result.stderr.ends_with(b"\n")
        && result.stderr.is_ascii()
    {
        Ok(json!({"error":std::str::from_utf8(&result.stderr[..result.stderr.len()-1])?}))
    } else {
        bail!(
            "unexpected native observer exit {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
    }
}
fn validate_row(row: &Value, wanted: &Value) -> Result<()> {
    ensure!(row == wanted, "{}: observed result differs", wanted["id"]);
    Ok(())
}
fn observe(exe: &Path, table: &Value) -> Result<Value> {
    let expected = cases(table)?;
    let mut rows = Vec::new();
    for (case, wanted) in table["cases"]
        .as_array()
        .context("missing cases")?
        .iter()
        .zip(expected.as_array().unwrap())
    {
        let raw = expand(&case["input"])?;
        let row = json!({"id":case["id"],"operation":case["operation"],"inputSha256":hash(&raw),
            "observed":observe_output(exchange(exe,case["operation"].as_str().context("missing operation")?,&raw)?)?});
        validate_row(&row, wanted)?;
        rows.push(row);
    }
    Ok(json!(rows))
}
fn collect(repo: &Path, out: &Path, table: &Value) -> Result<()> {
    ensure!(
        !out.exists() || fs::read_dir(out)?.next().is_none(),
        "expected empty output directory"
    );
    let rev = checked_text(
        Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD"]),
    )?;
    ensure!(
        rev == REV && table["sourceRevision"] == rev,
        "baseline observation needs the pinned Marmish revision"
    );
    ensure!(
        run(Command::new("git").current_dir(repo).args([
            "status",
            "--porcelain",
            "--untracked-files=all"
        ]))?
        .is_empty(),
        "observation requires a clean upstream tree"
    );
    let corpus = table["cases"].as_array().context("no cases")?;
    let ids: BTreeSet<_> = corpus.iter().map(|c| c["id"].as_str()).collect();
    ensure!(
        !corpus.is_empty() && ids.len() == corpus.len() && !ids.contains(&None),
        "expected unique case IDs"
    );
    let mut sources = BTreeMap::new();
    for group in [
        &table["sourceReferences"],
        &table["release"]["packagingInputs"],
        &json!(["Cargo.lock", "rust-toolchain.toml"]),
    ] {
        for path in group.as_array().context("missing sources")? {
            let p = path.as_str().context("invalid source path")?;
            sources.insert(p.to_string(), hash(&fs::read(repo.join(p))?));
        }
    }
    let lock: toml::Value = toml::from_str(&fs::read_to_string(repo.join("Cargo.lock"))?)?;
    let packages = lock["package"].as_array().context("missing packages")?;
    let core = packages
        .iter()
        .find(|p| p["name"].as_str() == Some("marmish-core"))
        .context("missing core")?;
    ensure!(
        core["dependencies"]
            .as_array()
            .context("missing dependencies")?
            .iter()
            .any(|d| d.as_str() == Some("base64 0.22.1"))
            && packages.iter().any(|p| p["name"].as_str() == Some("base64")
                && p["version"].as_str() == Some("0.22.1")
                && p.get("checksum").and_then(toml::Value::as_str) == Some(CODEC)),
        "unexpected base64 dependency"
    );
    let stage = tempfile::tempdir()?;
    let c = build(repo, stage.path())?;
    let tails = source("test_base64_tails.rs");
    let probe = stage.path().join("test-base64-tails");
    run(Command::new("rustc")
        .args(["+1.97.1", "--edition=2021", "--target", &c.host, "--extern"])
        .arg(format!("base64={}", c.base64.display()))
        .arg(&tails)
        .arg("-o")
        .arg(&probe))?;
    run(&mut Command::new(probe))?;
    let mut platforms = BTreeMap::new();
    for id in table["release"]["targets"]
        .as_object()
        .context("missing targets")?
        .keys()
    {
        platforms.insert(
            id,
            if id == &c.host {
                "core baseline observed"
            } else {
                "unobserved here"
            },
        );
    }
    let report = json!({"schemaVersion":1,"sourceRevision":rev,"contractSha256":hash(CONTRACT.as_bytes()),"consumerSha256":hash(&fs::read(source("observe_core.rs"))?),
        "rustc":c.compiler,"host":c.host,"canonicalBase64":{"version":"0.22.1","crateChecksum":CODEC,"testSha256":hash(&fs::read(tails)?),"paddedQuartets":266240},
        "sourceSha256":sources,"corePublicApi":"tested","cases":observe(&c.exe,table)?,"platforms":platforms,"releaseQualification":"unobserved","cliObservations":"native/baseline.json"});
    fs::create_dir_all(out)?;
    save(&out.join("results.json"), &report)
}
fn compare(original: &Path, candidate: &Path, out: &Path, table: &Value) -> Result<()> {
    let stage = tempfile::tempdir()?;
    let a = build(original, &stage.path().join("original"))?;
    let b = build(candidate, &stage.path().join("candidate"))?;
    let rows = observe(&a.exe, table)?;
    ensure!(
        rows == observe(&b.exe, table)?,
        "assembled codec observations differ from upstream"
    );
    let mut directions = Vec::new();
    for (writer, reader, from, to) in [
        (&a, &b, "original", "candidate"),
        (&b, &a, "candidate", "original"),
    ] {
        let mut witnesses = Vec::new();
        for c in table["cases"].as_array().unwrap() {
            if c["operation"] != "encode" || c["expected"].get("bytes").is_none() {
                continue;
            }
            let raw = expand(&c["input"])?;
            let frame = exchange(&writer.exe, "encode", &raw)?;
            ensure!(
                frame.status.success() && frame.stdout == expand(&c["expected"]["bytes"])?,
                "{from} emitted incorrect bytes"
            );
            let decoded = exchange(&reader.exe, "decode", &frame.stdout)?;
            ensure!(
                decoded.status.success() && decoded.stdout == raw,
                "{to} decoded incorrect bytes"
            );
            witnesses.push(
                json!({"id":c["id"],"wireSha256":hash(&frame.stdout),"rawSha256":hash(&raw)}),
            );
        }
        ensure!(!witnesses.is_empty(), "empty cross-version wire corpus");
        directions.push(json!({"writer":from,"reader":to,"exchanges":witnesses}));
    }
    save(
        out,
        &json!({"cases":rows,"directions":directions,"publicApi":{"original":"tested","candidate":"tested"}}),
    )
}
fn main() -> Result<()> {
    let table = serde_json::from_str(CONTRACT)?;
    match Args::parse().action {
        Action::Collect {
            upstream,
            empty_output,
        } => collect(&upstream, &empty_output, &table),
        Action::Compare {
            upstream,
            candidate,
            output,
        } => compare(&upstream, &candidate, &output, &table),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table() -> Value {
        serde_json::from_str(CONTRACT).unwrap()
    }
    fn case<'a>(t: &'a Value, id: &str) -> &'a Value {
        t["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap()
    }
    #[test]
    fn raw_boundary_and_wire() {
        let t = table();
        for (id, count) in [("encode-raw-limit", 16384), ("encode-raw-plus-one", 16385)] {
            assert_eq!(expand(&case(&t, id)["input"]).unwrap(), vec![255; count]);
        }
        // Three octets of FF produce four '/' digits. The final one or two FF octets use padding.
        for (count, suffix, id, key) in [
            (16384, "/w==", "encode-raw-limit", "expected"),
            (16385, "//8=", "decode-raw-plus-one", "input"),
        ] {
            let mut expr = &case(&t, id)[key];
            if key == "expected" {
                expr = &expr["bytes"];
            }
            let prefix = "/".repeat((count / 3) * 4);
            let expected = format!(r#"{{"version":1,"payload":"{prefix}{suffix}"}}"#);
            assert_eq!(expand(expr).unwrap(), expected.as_bytes());
        }
    }
    #[test]
    fn json_octets() {
        let t = table();
        for n in [32768, 32769] {
            let raw = expand(&case(&t, &format!("json-{n}"))["input"]).unwrap();
            assert_eq!(raw.len(), n);
            assert_eq!(
                serde_json::from_slice::<Value>(&raw).unwrap(),
                json!({"version":1,"payload":""})
            );
        }
        assert_eq!(
            case(&t, "json-32769")["expected"],
            json!({"error":"SizeLimit"})
        );
    }
    #[test]
    fn rejects_changed_precedence() {
        let expected = cases(&table()).unwrap();
        let wanted = expected
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "version-before-encoding")
            .unwrap();
        assert_eq!(wanted["observed"], json!({"error":"InvalidVersion"}));
        let mut wrong = wanted.clone();
        wrong["observed"] = json!({"error":"InvalidEncoding"});
        assert!(
            validate_row(&wrong, wanted)
                .unwrap_err()
                .to_string()
                .contains("version-before-encoding")
        );
    }
}
