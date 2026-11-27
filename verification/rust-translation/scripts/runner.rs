//! Stage the two pinned inputs, collect Kani logs, and replay the concrete guard witness.
use provider_validation::{Result, sha};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: u64 = 90;
const MUTANT: &str = "guard_priority_counterexample";
const PROVED: &str = "proved over the recorded domain";

fn require(ok: bool, message: impl Into<String>) -> Result {
    if ok {
        Ok(())
    } else {
        Err(message.into().into())
    }
}
fn version(command: &mut Command) -> Result<String> {
    let got = command.output()?;
    require(
        got.status.success(),
        format!("{command:?}: {}", String::from_utf8_lossy(&got.stderr)),
    )?;
    Ok(String::from_utf8(got.stdout)?.trim().into())
}
fn stage(src: &Path, expected: &str, dest: &Path) -> Result {
    let bytes = fs::read(src)?;
    require(
        format!("{:x}", Sha256::digest(&bytes)) == expected,
        format!("digest mismatch: {}", src.display()),
    )?;
    Ok(fs::write(dest, bytes)?)
}
fn mutate(path: &Path) -> Result<String> {
    let src = fs::read_to_string(path)?;
    let mut lines = src.split_inclusive('\n').collect::<Vec<_>>();
    let position = |name: &str| -> Result<usize> {
        let guard = format!("return Err(\"urn:oddlang:ledger:error:{name}\");");
        let mut found = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.contains(&guard));
        let position = found.next().map(|(i, _)| i);
        require(
            position.is_some() && found.next().is_none(),
            format!("expected one emitted guard for {name}"),
        )?;
        Ok(position.unwrap())
    };
    let first = position("SourceMissing")?;
    let moved = position("NonPositiveAmount")?;
    require(first < moved, "unexpected emitted guard order")?;
    let guard = lines.remove(moved);
    lines.insert(first, guard);
    fs::write(path, lines.concat())?;
    sha(path)
}
fn run(work: &Path, out: &Path, name: &str, command: &mut Command) -> Result<(ExitStatus, String)> {
    let path = out.join(format!("{name}.log"));
    let stream = fs::File::create(&path)?;
    let mut child = command
        .current_dir(work)
        .stdout(stream.try_clone()?)
        .stderr(stream)
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= Duration::from_secs(TIMEOUT + 90) {
            child.kill()?;
            child.wait()?;
            return Err(format!("{name} exceeded subprocess deadline").into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    Ok((status, fs::read_to_string(path)?))
}
fn kani(harness: &str) -> Command {
    let mut command = Command::new("cargo");
    command.args([
        "kani",
        "--harness",
        harness,
        "--exact",
        "--solver",
        "cadical",
        "-Z",
        "unstable-options",
        "--harness-timeout",
        "90s",
        "--output-format",
        "terse",
    ]);
    command
}
fn failed(log: &str, n: u8) -> bool {
    let prefix = format!("** {n} of ");
    log.split(&prefix).skip(1).any(|s| {
        let digits = s.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && !s.starts_with('0') && s[digits..].starts_with(" failed")
    })
}
fn success(status: ExitStatus, log: &str, covers: u8) -> Result {
    require(
        status.success()
            && log.contains("VERIFICATION:- SUCCESSFUL")
            && log.contains(&format!("{covers} of {covers} cover properties satisfied"))
            && failed(log, 0)
            && !log.contains("undetermined"),
        "Kani did not discharge all safety and cover checks",
    )
}
fn inventory(base: &Path, names: &[&str]) -> Result<Value> {
    let mut result = json!({});
    for name in names {
        result[name] = json!(sha(&base.join(name))?);
    }
    Ok(result)
}
fn main() -> Result {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let [original, generated, ohash, ghash, out] = args.as_slice() else {
        return Err(
            "expected ORIGINAL GENERATED ORIGINAL_SHA256 GENERATED_SHA256 OUTPUT_DIR".into(),
        );
    };
    let ohash = ohash
        .to_str()
        .ok_or("expected hexadecimal original digest")?;
    let ghash = ghash
        .to_str()
        .ok_or("expected hexadecimal generated digest")?;
    let out = Path::new(out);
    fs::create_dir_all(out)?;
    let out = fs::canonicalize(out)?;
    let result_file = out.join("result.json");
    match fs::remove_file(&result_file) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let backend = version(Command::new("cargo").args(["kani", "--version"]))?;
    require(
        backend
            .lines()
            .eq(["Kani Rust Verifier 0.68.0 (cargo plugin)", "CBMC 6.11.0"]),
        format!("unexpected verifier versions: {backend}"),
    )?;
    let compiler = env::var_os("KANI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap()).join(".kani/kani-0.68.0"));
    let compiler = version(Command::new(compiler.join("toolchain/bin/rustc")).arg("-vV"))?;
    require(
        compiler.contains("commit-hash: 8925ea358a0f265ca61026aadc7ecc506c545cbe"),
        "unexpected proof compiler",
    )?;
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("rust-translation");
    let package_hashes = inventory(
        &package,
        &[
            "Cargo.toml",
            "Cargo.lock",
            "src/lib.rs",
            "scripts/prove.sh",
            "scripts/runner.rs",
            "scripts/setup.sh",
        ],
    )?;
    let temp = tempfile::Builder::new()
        .prefix("kani-stage-")
        .tempdir_in(&out)?;
    let work = temp.path();
    fs::create_dir(work.join("src"))?;
    for rel in ["Cargo.toml", "Cargo.lock", "src/lib.rs"] {
        fs::copy(package.join(rel), work.join(rel))?;
    }
    stage(Path::new(original), ohash, &work.join("src/original.rs"))?;
    let staged = work.join("src/generated.rs");
    stage(Path::new(generated), ghash, &staged)?;
    let (status, log) = run(
        work,
        &out,
        "transfer_equivalence",
        &mut kani("transfer_equivalence"),
    )?;
    success(status, &log, 1)?;
    let (status, log) = run(
        work,
        &out,
        "populated_equivalence",
        &mut kani("populated_equivalence"),
    )?;
    let populated = if status.success() {
        success(status, &log, 7)?;
        PROVED
    } else {
        require(
            log.contains("CBMC timed out.") && !log.contains("Failed Checks:"),
            "populated harness failed for a reason other than its budget",
        )?;
        "inconclusive"
    };
    let mutant_hash = mutate(&staged)?;
    fs::copy(&staged, out.join("guard_mutant.rs"))?;
    let (status, log) = run(
        work,
        &out,
        "guard_mutant",
        kani(MUTANT).args(["-Z", "concrete-playback", "--concrete-playback", "inplace"]),
    )?;
    require(
        !status.success()
            && failed(&log, 1)
            && log.contains("Failed Checks: \"exact named result differs\"")
            && !log.contains("undetermined")
            && log.contains("VERIFICATION:- FAILED"),
        "expected the guard mutant assertion alone to fail",
    )?;
    let body = fs::read_to_string(work.join("src/lib.rs"))?;
    let prefix = "kani_concrete_playback_guard_priority_counterexample_";
    let suffix = body
        .split(&format!("fn {prefix}"))
        .skip(1)
        .filter_map(|s| s.split_once("()").map(|(n, _)| n))
        .find(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or("Kani did not emit witness function")?;
    require(
        body.contains("vec![255, 255, 255, 255, 255, 255, 255, 255]"),
        "Kani did not emit the signed -1 witness",
    )?;
    let test = format!("{prefix}{suffix}");
    let start = body
        .find("/// Test generated for harness ")
        .ok_or("missing Kani playback source")?;
    fs::write(out.join("guard_witness.rs"), &body[start..])?;
    let (status, log) = run(
        work,
        &out,
        "guard_replay",
        Command::new("cargo").args([
            "kani",
            "playback",
            "-Z",
            "concrete-playback",
            "--",
            "--exact",
            &test,
            "--nocapture",
        ]),
    )?;
    require(
        !status.success()
            && log.contains("test result: FAILED. 0 passed; 1 failed")
            && log.contains(&format!("test {test} ... FAILED"))
            && log.contains("\nexact named result differs\n"),
        "concrete playback did not reproduce the same assertion",
    )?;
    let artifacts = inventory(
        &out,
        &[
            "transfer_equivalence.log",
            "populated_equivalence.log",
            "guard_mutant.log",
            "guard_replay.log",
            "guard_witness.rs",
            "guard_mutant.rs",
        ],
    )?;
    let result = json!({"version":1,"originalSha256":ohash,"generatedSha256":ghash,"mutantSha256":mutant_hash,"packageSha256":package_hashes,"artifactsSha256":artifacts,
        "kani":"0.68.0","cbmc":"6.11.0","proofCompiler":compiler,"solver":"cadical","flags":["-C","overflow-checks=yes"],"unwind":64,"timeoutSeconds":TIMEOUT,
        "transfer_equivalence":PROVED,"populated_equivalence":populated,"guard_mutant":"disproved","guard_replay":"tested",
        "scope":"empty map, source s, destination s or d, arbitrary signed i64 amount",
        "populatedScope":"s/d/z rows, owners a/b/c, arbitrary admissible i64 balances, source s/x, destination s/x/d, arbitrary i64 amount",
        "unboundedKeysAndMaps":"inconclusive","witness":{"rows":[],"source":"s","destination":"d","amount":-1,"expected":"SourceMissing","actual":"NonPositiveAmount"}});
    let mut file = fs::File::create(result_file)?;
    serde_json::to_writer_pretty(&mut file, &result)?;
    Ok(file.write_all(b"\n")?)
}
