//! Compare native and Wasm batch observers with the independent framing contract.
use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use marmish_inventory::{PIN, digest, file_hashes, read, read_json, root};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Read, Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[path = "../framing_contract.rs"]
mod framing_contract;
#[path = "../frame_inputs.rs"]
mod inputs;
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    VerifyInputs {
        imported: PathBuf,
        projected: PathBuf,
        mutant: PathBuf,
    },
    Compare {
        out: PathBuf,
        host: PathBuf,
        imported: PathBuf,
        projected: PathBuf,
        mutant: PathBuf,
    },
}
fn requests(cases: &[Value]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for case in cases {
        let input = framing_contract::expand(&case["input"])?;
        bytes.push(match case["operation"].as_str() {
            Some("encode") => b'E',
            Some("decode") => b'D',
            _ => bail!("unknown operation"),
        });
        bytes.extend(u32::try_from(input.len())?.to_le_bytes());
        bytes.extend(input);
    }
    Ok(bytes)
}
fn responses(raw: &[u8], cases: &[Value]) -> Result<Vec<Value>> {
    let mut stream = Cursor::new(raw);
    let mut results = Vec::new();
    for case in cases {
        let mut header = [0u8; 5];
        stream
            .read_exact(&mut header)
            .context("incomplete framing observation")?;
        let size = u32::from_le_bytes(header[1..].try_into()?) as usize;
        let start = usize::try_from(stream.position())?;
        let end = start
            .checked_add(size)
            .context("invalid observation length")?;
        let data = raw
            .get(start..end)
            .context("incomplete framing observation")?;
        stream.set_position(end as u64);
        let value = match header[0] {
            b'B' => json!({"bytesHex":framing_contract::hex(data)}),
            b'F' if data.is_ascii() => json!({"error":std::str::from_utf8(data)?}),
            _ => bail!("unknown observation type or invalid error"),
        };
        results.push(json!({"id":case["id"],"operation":case["operation"],"inputSha256":digest(&framing_contract::expand(&case["input"])?),"observed":value}));
    }
    ensure!(
        stream.position() == raw.len() as u64,
        "unexpected additional observation"
    );
    Ok(results)
}
fn batch(cmd: &mut Command, cases: &[Value]) -> Result<Vec<Value>> {
    // File-backed standard I/O lets the parent enforce the historical 180-second deadline.
    let mut input = tempfile::tempfile()?;
    input.write_all(&requests(cases)?)?;
    input.rewind()?;
    let mut output = tempfile::tempfile()?;
    let mut child = cmd
        .stdin(Stdio::from(input))
        .stdout(Stdio::from(output.try_clone()?))
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "observer exited {status}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait()?;
            bail!("framing observer timed out");
        }
        thread::sleep(Duration::from_millis(15));
    }
    output.rewind()?;
    let mut raw = Vec::new();
    output.read_to_end(&mut raw)?;
    responses(&raw, cases)
}
fn save(path: &Path, v: &Value) -> Result<String> {
    let mut data = serde_json::to_vec_pretty(v)?;
    data.push(b'\n');
    fs::write(path, &data)?;
    Ok(digest(&data))
}
fn compare(
    out: &Path,
    host: &Path,
    imported: &Path,
    projected: &Path,
    mutant: &Path,
) -> Result<()> {
    inputs::verify_inputs(imported, projected, mutant)?;
    let contract = read(&root().join("examples/marmish/observations.json"))?;
    let table: Value = serde_json::from_slice(&contract)?;
    let cases = table["cases"].as_array().context("missing cases")?;
    let wanted = framing_contract::cases(&table)?;
    let raw = read(&out.join("original/results.json"))?;
    let original: Value = serde_json::from_slice(&raw)?;
    ensure!(
        original["sourceRevision"] == PIN && original["contractSha256"] == digest(&contract),
        "stale original observations"
    );
    let lock = read_json(&imported.join("import.lock"))?;
    let observed = &original["sourceSha256"];
    ensure!(
        lock["build"]["inputs"]["src/frame.rs"] == observed["crates/marmish-core/src/frame.rs"]
            && lock["build"]["cargoLockSha256"] == observed["Cargo.lock"],
        "framing source or lock differs"
    );
    ensure!(
        original["cases"] == wanted,
        "original disagrees with independent expectations"
    );
    let mut builds = json!({});
    for (name, src) in [("regenerated", projected), ("mutant", mutant)] {
        let dest = out.join(name);
        let mut results = json!({});
        for role in ["native", "wasm"] {
            let mut cmd = if role == "native" {
                Command::new(dest.join("consumer"))
            } else {
                let mut c = Command::new(host);
                c.arg(dest.join("component.wasm"));
                c
            };
            let actual = batch(&mut cmd, cases)?;
            let differences:Vec<_>=wanted.as_array().unwrap().iter().zip(&actual).filter(|(a,b)|a!=b)
                .map(|(reference,row)|json!({"case":row["id"],"expected":reference["observed"],"actual":row["observed"]})).collect();
            let required = if name == "mutant" {
                json!([{"case":"version-before-size","expected":{"error":"InvalidVersion"},"actual":{"error":"SizeLimit"}}])
            } else {
                json!([])
            };
            ensure!(
                json!(differences) == required,
                "{name} {role} unexpected differences"
            );
            let identity = save(
                &dest.join(format!("{role}.json")),
                &json!({"cases":actual,"differences":differences}),
            )?;
            results[role] = json!({"observationsSha256":identity,"caseCount":actual.len(),"differences":differences});
        }
        builds[name] = json!({"projection":file_hashes(src,&["projection.json","frame.rs","world.wit"])?,
            "artifacts":file_hashes(&dest,&["consumer","component.wasm","extracted.wit"])?,"wasmImports":[],"results":results});
    }
    save(
        &out.join("report.json"),
        &json!({"schemaVersion":1,"sourceRevision":PIN,"classification":"tested",
        "contractSha256":digest(&contract),"originalSha256":digest(&raw),"graphSha256":digest(&read(&imported.join("domain.ttl"))?),
        "importLockSha256":digest(&read(&imported.join("import.lock"))?),"hostSha256":digest(&read(host)?),"rustc":fs::read_to_string(out.join("rustc.txt"))?.trim(),
        "wasmTools":fs::read_to_string(out.join("wasm-tools.txt"))?.trim(),"harnessSha256":inputs::harness(&root())?,"cases":cases.len(),"builds":builds}),
    )?;
    Ok(())
}
fn main() -> Result<()> {
    match Args::parse().action {
        Action::VerifyInputs {
            imported,
            projected,
            mutant,
        } => inputs::verify_inputs(&imported, &projected, &mutant),
        Action::Compare {
            out,
            host,
            imported,
            projected,
            mutant,
        } => compare(&out, &host, &imported, &projected, &mutant),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinct_bytes_and_errors() -> Result<()> {
        let cases = [json!({"id":"binary","operation":"decode","input":[{"hex":"00ff"}]})];
        assert_eq!(requests(&cases)?, b"D\x02\0\0\0\0\xff");
        assert_eq!(
            responses(b"B\x04\0\0\0Oops", &cases)?[0]["observed"],
            json!({"bytesHex":"4f6f7073"})
        );
        assert_eq!(
            responses(b"F\x04\0\0\0Oops", &cases)?[0]["observed"],
            json!({"error":"Oops"})
        );
        Ok(())
    }
    #[test]
    fn incomplete_extra_and_unknown() {
        let cases = [json!({"id":"binary","operation":"decode","input":[]})];
        for data in [&b""[..], b"B\x01\0\0\0", b"B\0\0\0\0extra", b"X\0\0\0\0"] {
            assert!(responses(data, &cases).is_err());
        }
    }
}
