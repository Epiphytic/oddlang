//! Recompute each translation claim and its witnesses against the supplied source revision.
use provider_validation::{Result, read, sha};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

const FIXTURE: &str = "examples/rust-import/ledger";
const ERROR: &str = "urn:oddlang:ledger:error:";
const HARNESS: &str = "tests/rust_translation.rs";
const ORACLE: &str = "tests/support/ledger_oracle.rs";
const VERIFIER: &str = "verification/provider-validation/src/bin/translation-evidence.rs";
const PROVED: &str = "proved over the recorded domain";

fn require(ok: bool, message: impl Into<String>) -> Result {
    if ok {
        Ok(())
    } else {
        Err(message.into().into())
    }
}
fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or_else(|| "expected string".into())
}
fn integer(value: &Value) -> Result<i64> {
    value.as_i64().ok_or_else(|| "expected i64".into())
}
fn object(value: &Value) -> Result<&Map<String, Value>> {
    value.as_object().ok_or_else(|| "expected object".into())
}
fn outcomes() -> BTreeSet<String> {
    [
        "SourceMissing",
        "DestinationMissing",
        "SameAccount",
        "NonPositiveAmount",
        "InsufficientFunds",
        "DestinationOverflow",
    ]
    .into_iter()
    .map(|name| format!("{ERROR}{name}"))
    .chain([String::from("ok")])
    .collect()
}
fn inventory<S: AsRef<str>>(root: &Path, names: impl IntoIterator<Item = S>) -> Result<Value> {
    let mut map = Map::new();
    for name in names {
        let name = name.as_ref();
        map.insert(name.into(), json!(sha(&root.join(name))?));
    }
    Ok(Value::Object(map))
}
fn canonical(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn check_lock(root: &Path, report: &Path) -> Result<Value> {
    let fixture = root.join(FIXTURE);
    let lock = read(&report.join("import/import.lock"))?;
    let overlays = ["avro.ttl", "fields.ttl", "rust.ttl", "sql.ttl"];
    require(
        lock["inputs"] == inventory(&fixture, ["Cargo.toml", "Cargo.lock", "src/lib.rs"])?,
        "locked source inputs changed",
    )?;
    let mut frozen = Map::new();
    for line in fs::read_to_string(fixture.join("original.sha256"))?.lines() {
        let (hash, name) = line
            .split_once("  ")
            .ok_or("invalid original digest list")?;
        frozen.insert(name.into(), json!(hash));
    }
    require(
        lock["inputs"] == Value::Object(frozen),
        "frozen source digest disagrees with import lock",
    )?;
    require(
        lock["overlays"] == inventory(&fixture.join("overlays"), overlays)?,
        "locked overlays changed",
    )?;
    require(
        lock["files"]
            == inventory(
                &report.join("import"),
                overlays
                    .into_iter()
                    .chain(["domain.ttl", "operations.ttl", "source-map.json"]),
            )?,
        "imported artifacts changed",
    )?;
    let semantic = object(&lock["files"])?
        .iter()
        .filter(|(name, _)| name.ends_with(".ttl"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    require(
        lock["graphSha256"]
            == format!("{:x}", Sha256::digest(canonical(&Value::Object(semantic))?)),
        "graph digest disagrees with the imported ontology",
    )?;
    require(
        lock["mappingSha256"] == sha(&fixture.join("mapping.toml"))?,
        "mapping changed",
    )?;
    require(
        lock["profile"] == "ledger-v1"
            && lock["version"] == 1
            && lock.get("cleanup") == Some(&Value::Null),
        "unexpected import profile",
    )?;
    require(
        sha(&report.join("original.pin"))? == sha(&fixture.with_extension("baseline"))?,
        "original baseline changed",
    )?;
    require(
        !string(&lock["target"])?.is_empty()
            && string(&lock["compiler"])?.contains("commit-hash: ")
            && lock["flags"] == json!(["-C", "overflow-checks=yes"]),
        "missing production configuration",
    )?;
    Ok(lock)
}
fn mutants(value: &Value) -> Result {
    let guard = &value["guardOrder"];
    let entry = &guard["input"];
    let rows = &entry["rows"];
    require(
        !object(rows)?.contains_key(string(&entry["source"])?) && integer(&entry["amount"])? < 0,
        "guard witness lacks overlapping missing-source and negative-amount guards",
    )?;
    let observation =
        |name: &str, rows: &Value| json!({"status":format!("{ERROR}{name}"),"rows":rows});
    require(
        guard["expected"] == observation("SourceMissing", rows)
            && guard["actual"] == observation("NonPositiveAmount", rows),
        "guard mutant witness does not discriminate error order",
    )?;
    let debit = &value["earlyDebit"];
    let entry = &debit["input"];
    let rows = &entry["rows"];
    let src = string(&entry["source"])?;
    let dst = string(&entry["destination"])?;
    let amount = integer(&entry["amount"])?;
    let balance = integer(&rows[src]["balance"])?;
    require(
        src != dst
            && amount > 0
            && amount <= balance
            && integer(&rows[dst]["balance"])?
                .checked_add(amount)
                .is_none(),
        "invalid overflow witness",
    )?;
    let mut changed = rows.clone();
    changed[src]["balance"] = json!(balance - amount);
    require(
        debit["expected"] == observation("DestinationOverflow", rows)
            && debit["actual"] == observation("DestinationOverflow", &changed),
        "early debit witness does not show the premature state change",
    )
}
fn hex(text: &str) -> String {
    text.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn differential(root: &Path, report: &Path, mut hashes: Value, lock: &Value) -> Result<Value> {
    let diff = read(&report.join("differential.json"))?;
    require(
        diff["classification"] == "tested"
            && diff["schemaVersion"] == 1
            && diff["role"] == "native-rust",
        "unexpected differential claim",
    )?;
    require(
        diff["operation"] == "urn:oddlang:ledger:TransferFunds",
        "wrong differential operation",
    )?;
    hashes["adapter"] = json!(sha(&root.join("tests/support/rust_translation/runner.rs"))?);
    hashes["corpusSource"] = json!(sha(&root.join("tests/support/rust_translation/corpus.rs"))?);
    hashes["graph"] = lock["graphSha256"].clone();
    hashes["importLock"] = json!(sha(&report.join("import/import.lock"))?);
    for (key, file) in [
        ("cases", "cases"),
        ("reorderedSource", "reordered-source.rs"),
        ("reorderedGenerated", "reordered-generated.rs"),
        ("earlyDebitGenerated", "early-debit.rs"),
    ] {
        hashes[key] = json!(sha(&report.join("corpus").join(file))?);
    }
    require(
        object(&diff["artifacts"])?
            .keys()
            .eq(object(&hashes)?.keys()),
        "incomplete differential artifact inventory",
    )?;
    for (k, v) in object(&hashes)? {
        require(
            &diff["artifacts"][k] == v,
            format!("stale differential artifact: {k}"),
        )?;
    }
    let counts = object(&diff["outcomes"])?;
    require(
        counts.keys().cloned().collect::<BTreeSet<_>>() == outcomes()
            && counts.values().all(|n| n.as_u64().is_some_and(|n| n > 0)),
        "missing reachable outcome witness",
    )?;
    let corpus = fs::read_to_string(report.join("corpus/cases"))?;
    let lines = corpus.lines().collect::<Vec<_>>();
    let total = counts
        .values()
        .map(|n| u128::from(n.as_u64().unwrap()))
        .sum::<u128>();
    require(
        total == lines.len() as u128 && diff["cases"] == json!(lines.len()),
        "case count disagrees with corpus",
    )?;
    require(
        diff["randomCases"]
            .as_u64()
            .is_some_and(|n| n > 0 && n < lines.len() as u64),
        "missing seeded or hand-selected cases",
    )?;
    let seed = string(&diff["seed"])?.trim().trim_start_matches('+');
    let digits = seed
        .strip_prefix("0x")
        .or_else(|| seed.strip_prefix("0X"))
        .unwrap_or(seed);
    require(
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit()),
        "missing corpus seed",
    )?;
    let cleanup = &diff["cleanup"];
    require(
        cleanup["originalSha256"] == hashes["original"]
            && cleanup["effectiveSha256"] != hashes["original"]
            && cleanup["cases"] == diff["cases"],
        "edited-source comparison incomplete",
    )?;
    for (key, file) in [
        ("effectiveSha256", "effective.rs"),
        ("generatedSha256", "cleaned.rs"),
        ("patchSha256", "cleanup.patch"),
    ] {
        require(
            cleanup[key] == sha(&report.join("corpus").join(file))?,
            format!("stale cleanup artifact: {file}"),
        )?;
    }
    mutants(&diff["mutants"])?;
    for mutant in object(&diff["mutants"])?.values() {
        let entry = &mutant["input"];
        let mut fields = vec![
            hex(string(&entry["source"])?),
            hex(string(&entry["destination"])?),
            integer(&entry["amount"])?.to_string(),
        ];
        for (key, row) in object(&entry["rows"])? {
            require(row["id"] == *key, "invalid witness key")?;
            fields.extend([
                hex(key),
                hex(string(&row["owner"])?),
                integer(&row["balance"])?.to_string(),
            ]);
        }
        require(
            lines.contains(&fields.join("\t").as_str()),
            "mutant input absent from archived corpus",
        )?;
    }
    Ok(diff)
}
fn failed_count(log: &str, failed: u8) -> bool {
    let prefix = format!("** {failed} of ");
    log.match_indices(&prefix).any(|(start, _)| {
        let rest = &log[start + prefix.len()..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && !rest.starts_with('0') && rest[digits..].starts_with(" failed")
    })
}
fn proved_log(log: &str, name: &str, covers: u8) -> Result {
    require(
        log.contains(&format!("Checking harness {name}..."))
            && failed_count(log, 0)
            && log.contains(&format!("{covers} of {covers} cover properties satisfied"))
            && log.contains("VERIFICATION:- SUCCESSFUL")
            && log.contains("Complete - 1 successfully verified harnesses, 0 failures")
            && !log.contains("undetermined"),
        format!("incomplete successful proof: {name}"),
    )
}
fn proof(root: &Path, report: &Path, hashes: &Value) -> Result<Value> {
    let folder = report.join("proof");
    let proof = read(&folder.join("result.json"))?;
    let pins = json!({
        "version":1,"kani":"0.68.0","cbmc":"6.11.0","solver":"cadical","unwind":64,"flags":["-C","overflow-checks=yes"],"timeoutSeconds":90,
        "originalSha256":hashes["original"],"generatedSha256":hashes["generated"],
        "transfer_equivalence":PROVED,"guard_mutant":"disproved","guard_replay":"tested","unboundedKeysAndMaps":"inconclusive",
        "scope":"empty map, source s, destination s or d, arbitrary signed i64 amount",
        "populatedScope":"s/d/z rows, owners a/b/c, arbitrary admissible i64 balances, source s/x, destination s/x/d, arbitrary i64 amount",
        "witness":{"rows":[],"source":"s","destination":"d","amount":-1,"expected":"SourceMissing","actual":"NonPositiveAmount"}
    });
    for (k, v) in object(&pins)? {
        require(
            &proof[k] == v,
            format!("invalid proof configuration or claim: {k}"),
        )?;
    }
    require(
        string(&proof["proofCompiler"])?
            .contains("commit-hash: 8925ea358a0f265ca61026aadc7ecc506c545cbe"),
        "wrong proof compiler",
    )?;
    let package = root.join("verification/rust-translation");
    let package_files = [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "scripts/prove.sh",
        if package.join("scripts/runner.rs").is_file() {
            "scripts/runner.rs"
        } else {
            "scripts/runner.py"
        },
        "scripts/setup.sh",
    ];
    require(
        proof["packageSha256"] == inventory(&package, package_files)?,
        "stale proof package",
    )?;
    require(
        proof["artifactsSha256"]
            == inventory(
                &folder,
                [
                    "transfer_equivalence.log",
                    "populated_equivalence.log",
                    "guard_mutant.log",
                    "guard_replay.log",
                    "guard_witness.rs",
                    "guard_mutant.rs",
                ],
            )?,
        "stale proof output",
    )?;
    require(
        proof["mutantSha256"] == sha(&folder.join("guard_mutant.rs"))?,
        "stale guard mutant",
    )?;
    require(
        fs::read(folder.join("guard_mutant.rs"))?
            == fs::read(report.join("corpus/reordered-generated.rs"))?,
        "proof mutant differs from independently regenerated guard-order mutant",
    )?;
    proved_log(
        &fs::read_to_string(folder.join("transfer_equivalence.log"))?,
        "transfer_equivalence",
        1,
    )?;
    let populated = fs::read_to_string(folder.join("populated_equivalence.log"))?;
    if proof["populated_equivalence"] == PROVED {
        proved_log(&populated, "populated_equivalence", 7)?;
    } else {
        require(
            proof["populated_equivalence"] == "inconclusive"
                && populated.contains("Checking harness populated_equivalence...")
                && populated.contains("CBMC timed out.")
                && !populated.contains("Failed Checks:"),
            "unexplained populated proof result",
        )?;
    }
    let mutant = fs::read_to_string(folder.join("guard_mutant.log"))?;
    require(
        mutant.contains("Checking harness guard_priority_counterexample...")
            && failed_count(&mutant, 1)
            && mutant.contains("Failed Checks: \"exact named result differs\"")
            && !mutant.contains("undetermined")
            && mutant.contains("VERIFICATION:- FAILED"),
        "unexpected guard mutant failure",
    )?;
    let witness = fs::read_to_string(folder.join("guard_witness.rs"))?;
    let replay = fs::read_to_string(folder.join("guard_replay.log"))?;
    let prefix = "kani_concrete_playback_guard_priority_counterexample_";
    let matching = witness
        .split(&format!("fn {prefix}"))
        .skip(1)
        .filter_map(|suffix| suffix.split_once("()").map(|(n, _)| n))
        .find(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    require(
        matching.is_some_and(|n| replay.contains(&format!("test {prefix}{n} ... FAILED")))
            && witness.contains("vec![255, 255, 255, 255, 255, 255, 255, 255]")
            && replay.contains("test result: FAILED. 0 passed; 1 failed")
            && replay.contains("\nexact named result differs\n"),
        "missing concrete Kani replay",
    )?;
    Ok(json!({
        "emptyMap":{"classification":PROVED,"domain":proof["scope"]},
        "populatedMap":{"classification":proof["populated_equivalence"],"domain":proof["populatedScope"]},
        "guardKaniCounterexample":{"classification":"disproved","witness":proof["witness"]},
        "guardConcretePlayback":{"classification":"tested","witness":"proof/guard_witness.rs"}
    }))
}
fn summarize(root: &Path, report: &Path) -> Result<Value> {
    let lock = check_lock(root, report)?;
    let hashes = json!({"original":sha(&root.join(FIXTURE).join("src/lib.rs"))?,
        "generated":sha(&report.join("generated/account.rs"))?,"oracle":sha(&root.join(ORACLE))?,"harness":sha(&root.join(HARNESS))?});
    let diff = differential(root, report, hashes.clone(), &lock)?;
    let mut claims = proof(root, report, &hashes)?;
    let mut inputs = vec![
        "Cargo.toml",
        "Cargo.lock",
        HARNESS,
        ORACLE,
        "docs/language/ledger-core.md",
        "scripts/verify-rust-translation.sh",
        "verification/rust-translation/README.md",
        "examples/rust-import/ledger/original.sha256",
        "examples/rust-import/ledger.baseline",
    ];
    // Historical receipts inventory the verifier shipped in their source revision.
    if root.join(VERIFIER).is_file() {
        inputs.extend([
            VERIFIER,
            "verification/provider-validation/src/lib.rs",
            "verification/provider-validation/Cargo.toml",
            "verification/provider-validation/Cargo.lock",
        ]);
    } else {
        inputs.push("verification/translation-evidence/verify.py");
    }
    let mut artifacts = [
        "original.pin",
        "import/import.lock",
        "differential.json",
        "proof/result.json",
        "generated/account.rs",
        "generated/account.avsc",
        "generated/schema.sql",
    ]
    .map(str::to_owned)
    .to_vec();
    artifacts.extend(
        object(&lock["files"])?
            .keys()
            .map(|k| format!("import/{k}")),
    );
    claims["nativeCorpus"] =
        json!({"classification":"tested","cases":diff["cases"],"seed":diff["seed"]});
    claims["editedSourceCorpus"] =
        json!({"classification":"tested","cases":diff["cleanup"]["cases"]});
    claims["guardOrderMutant"] =
        json!({"classification":"disproved","witness":"differential.json#mutants/guardOrder"});
    claims["earlyDebitMutant"] =
        json!({"classification":"disproved","witness":"differential.json#mutants/earlyDebit"});
    claims["arbitraryLedger"] = json!({"classification":"inconclusive","domain":"All valid ledgers, arbitrary map sizes, keys and owner strings.",
        "openObligations":["Key renaming preserves presence, value, and all observable results in both bodies.",
        "Adding arbitrary bystanders preserves addressed results and every bystander.","The proof configuration models the production toolchain for the claimed domain."]});
    Ok(json!({"schemaVersion":1,
        "contract":{"profile":lock["profile"],"entity":"urn:oddlang:ledger:Account","operation":"urn:oddlang:ledger:TransferFunds","outcomes":outcomes()},
        "graphSha256":lock["graphSha256"],"production":{"compiler":lock["compiler"],"target":lock["target"],"flags":lock["flags"]},
        "observations":["success or exact first named error","all map keys","id, owner and balance of every poststate row, including bystanders"],
        "roles":["native-rust"],"proofAssumptions":["Single-threaded calls, successful allocation and completed Rust observations.",
            "Kani MIR and standard-library models, abort on panic, checked overflow and unwinding assertions.",
            "The proof covers Kani's pinned compiler and target. Equivalence with native compilation remains open."],
        "sourceInputs":inventory(root,inputs)?,"reportFiles":inventory(report,artifacts)?,"claims":claims}))
}
fn check(root: &Path, report: &Path, write: bool) -> Result {
    let result = canonical(&summarize(root, report)?)?;
    let path = report.join("summary.json");
    if write {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(&result)?;
    }
    require(
        fs::read(path)? == result,
        "summary.json differs from recomputed evidence",
    )
}
fn main() -> Result {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let (root, report, write) = match args.as_slice() {
        [root, report] => (root, report, false),
        [root, report, flag] if flag == "--write" => (root, report, true),
        _ => return Err("expected SOURCE_ROOT REPORT [--write]".into()),
    };
    check(&PathBuf::from(root), &PathBuf::from(report), write)
}
#[cfg(test)]
#[path = "translation_evidence/tests.rs"]
mod tests;
