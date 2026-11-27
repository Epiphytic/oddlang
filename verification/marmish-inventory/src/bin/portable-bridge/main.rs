//! Replay only the frozen local bridge argument, initialization and EOF cases.
mod locks;
mod process;
mod state;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use marmish_inventory::{array, digest, read, root, strings, text};
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
const BASELINE_SHA: &str = "08dd1637bc59b9b17571f269d538b8ef07e1ed931f8a78e558b2c88e60ee7272";
#[derive(Parser)]
struct Args {
    bridge: PathBuf,
    worker: PathBuf,
    #[arg(long)]
    report: Option<PathBuf>,
}
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}
fn path_literal(path: &Path) -> Result<String> {
    let mut ascii = String::new();
    for ch in serde_json::to_string(path)?.chars() {
        if ch < '\u{7f}' {
            ascii.push(ch)
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                write!(ascii, "\\u{unit:04x}")?;
            }
        }
    }
    Ok(ascii)
}
fn request(
    baseline: &Value,
    ex: &Value,
    cwd: &Path,
    worker: &Path,
    state: &Path,
) -> Result<String> {
    let mut result = String::new();
    for (i, part) in text(&baseline["request"])?.split('@').enumerate() {
        if i % 2 == 0 {
            result.push_str(part)
        } else {
            let value = match part {
                "STATE" => path_literal(state)?,
                "CWD" => path_literal(cwd)?,
                "WORKER" => path_literal(worker)?,
                "VERSION" => ex["version"].as_i64().context("init version")?.to_string(),
                _ => anyhow::bail!("unknown init field"),
            };
            result.push_str(&value);
        }
    }
    Ok(result)
}
fn process_difference(expected: &Value, got: &Value) -> Option<&'static str> {
    ["status", "stdout_after_eof", "stderr"]
        .into_iter()
        .find(|field| expected[*field] != got[*field])
}
fn run_case(baseline: &Value, case: &Value, bridge: &Path, worker: &Path) -> Result<Value> {
    let temp = tempfile::Builder::new()
        .prefix("oddlang-bridge-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")?;
    let dir = temp.path().canonicalize()?;
    let cwd = dir.join("cwd");
    fs::create_dir(&cwd)?;
    fs::set_permissions(&cwd, fs::Permissions::from_mode(0o700))?;
    let (states, replies) = (&baseline["states"], &baseline["replies"]);
    let (initial, mut last) = state::inspect(&dir)?;
    state::compare(&states["absent"], &initial)?;
    let mut child = process::Session::start(bridge, &strings(&case["args"])?, &cwd)?;
    let mut exchanges = vec![];
    for ex in array(&case["exchanges"])? {
        let req = request(baseline, ex, &cwd, worker, &dir.join("state"))?;
        let reply = latin1(&child.exchange(req.as_bytes())?);
        ensure!(
            replies[text(&ex["reply"])?] == reply,
            "stdout before EOF differs"
        );
        let (observed, hashes) = state::inspect(&dir)?;
        state::compare(&states[text(&ex["state"])?], &observed)?;
        let mut row = json!({"stdin":req,"stdout":reply,"state":observed});
        if ex["unchanged"] == true {
            ensure!(last == hashes, "state bytes changed on rejected init");
            row["contents_unchanged"] = json!(true);
        }
        last = hashes;
        exchanges.push(row);
    }
    let tail = text(&case["stdin_before_eof"])?;
    let bytes = tail
        .chars()
        .map(|c| u8::try_from(c as u32))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let (status, out, err) = child.eof(&bytes)?;
    let (final_state, hashes) = state::inspect(&dir)?;
    state::compare(&states[text(&case["final_state"])?], &final_state)?;
    let retained = if let Some(before) = last.get("state/engine.key") {
        ensure!(
            hashes.get("state/engine.key") == Some(before),
            "key changed after EOF"
        );
        Some(true)
    } else {
        None
    };
    let result = json!({
        "id":case["id"],"args":case["args"],"cwd":cwd,"initial_state":initial,"exchanges":exchanges,
        "stdin_before_eof":tail,"stdout_after_eof":latin1(&out),"stderr":latin1(&err),"status":status,
        "final_state":final_state,"key_retained_after_eof":retained
    });
    if let Some(field) = process_difference(case, &result) {
        anyhow::bail!("process {field} differs")
    }
    Ok(result)
}
fn controls(baseline: &Value, rows: &[Value]) -> Result<Value> {
    let cases = array(&baseline["cases"])?;
    let mut verdict = json!({});
    for (id, key, field) in [
        ("argument-rejection", "expected_status", "status"),
        ("partial-frame-at-eof", "wrong_channel", "stdout_after_eof"),
    ] {
        let mut expected = cases
            .iter()
            .find(|c| c["id"] == id)
            .context("missing control case")?
            .clone();
        let observed = rows
            .iter()
            .find(|r| r["id"] == id)
            .context("missing control observation")?;
        if field == "status" {
            expected["status"] = json!(0);
        } else {
            expected["stderr"] = expected["stdout_after_eof"].clone();
            expected["stdout_after_eof"] = json!("");
        }
        ensure!(
            process_difference(&expected, observed) == Some(field),
            "undetected control: {key}"
        );
        verdict[key] = json!("rejected");
    }
    Ok(verdict)
}
fn baseline() -> Result<Value> {
    let bytes = read(&root().join("examples/marmish/portable-bridge/baseline.json"))?;
    ensure!(
        digest(&bytes) == BASELINE_SHA,
        "local bridge fixture commitment differs"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn main() -> Result<()> {
    let a = Args::parse();
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "pinned bridge requires an unprivileged account"
    );
    let bridge = a.bridge.canonicalize()?;
    let worker = a.worker.canonicalize()?;
    let base = baseline()?;
    let binaries = || -> Result<Value> {
        Ok(json!({"bridge":digest(&read(&bridge)?),"worker":digest(&read(&worker)?)}))
    };
    let pins = binaries()?;
    let startup = process::limits()?;
    let mut cases = vec![];
    for case in array(&base["cases"])? {
        cases.push(
            run_case(&base, case, &bridge, &worker)
                .with_context(|| format!("bridge case {}", case["id"]))?,
        );
    }
    let controls = controls(&base, &cases)?;
    ensure!(
        pins == binaries()?,
        "bridge or worker changed during replay"
    );
    let mut report = json!({"format":1,"fixture_sha256":BASELINE_SHA,"binary_sha256":pins,"environment":{"PATH":"/usr/bin:/bin","LANG":"C"},"umask":"0077","startup_limits":startup,"cases":cases,"controls":controls});
    if let Some(path) = a.report {
        report.sort_all_objects();
        fs::write(path, serde_json::to_string_pretty(&report)? + "\n")?;
    }
    println!(
        "{} bridge CLI cases passed. Changed status and output channel rejected.",
        cases.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archived_transcript_and_field_controls() -> Result<()> {
        let base = baseline()?;
        let archive = marmish_inventory::read_json(
            &root().join("examples/marmish/portable-bridge/original.json"),
        )?;
        ensure!(
            archive["fixture_sha256"] == BASELINE_SHA,
            "fixture mismatch"
        );
        let observations = array(&archive["cases"])?;
        let wanted = array(&base["cases"])?;
        assert_eq!(wanted.len(), observations.len());
        for (case, r) in wanted.iter().zip(observations) {
            assert_eq!(case["id"], r["id"]);
            assert_eq!(process_difference(case, r), None);
            let states = &base["states"];
            state::compare(&states["absent"], &r["initial_state"])?;
            state::compare(&states[text(&case["final_state"])?], &r["final_state"])?;
            for (ex, obs) in array(&case["exchanges"])?
                .iter()
                .zip(array(&r["exchanges"])?)
            {
                assert_eq!(base["replies"][text(&ex["reply"])?], obs["stdout"]);
                state::compare(&states[text(&ex["state"])?], &obs["state"])?;
            }
        }
        assert_eq!(controls(&base, observations)?, archive["controls"]);
        Ok(())
    }
}
