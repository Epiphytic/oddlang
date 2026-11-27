use super::*;
use anyhow::bail;
use std::collections::BTreeMap;
pub mod collect;
pub mod installed;
pub mod original;
mod transcripts;
const TARGET: &str = "aarch64-apple-darwin";
const BINS: [&str; 3] = ["marmish", "marmishd", "marmish-worker"];
// The historical Python comparator and the published 6c8480a Rust replay each
// retain the identity stamped on their original reports.
pub const LEGACY_VERIFIER: &str =
    "1f7c5c311dc8f71c8b43927f048ec27d396ae94476650a5ef853da057ae92205";
/// Identity of the Rust comparison implementation used to produce a fresh receipt.
pub fn verifier() -> String {
    let mut hash = Sha256::new();
    for source in [
        include_bytes!("../lib.rs").as_slice(),
        include_bytes!("mod.rs"),
        include_bytes!("collect.rs"),
        include_bytes!("original.rs"),
        include_bytes!("installed.rs"),
        include_bytes!("transcripts.rs"),
        include_bytes!("../../Cargo.lock"),
    ] {
        hash.update((source.len() as u64).to_be_bytes());
        hash.update(source);
    }
    format!("{:x}", hash.finalize())
}
pub fn compare(
    dir: &Path,
    original_commit: &str,
    revision: &str,
    tree: &str,
    verifier_hash: &str,
) -> Result<Value> {
    ensure!(original_commit == PIN, "wrong archived original identity");
    ensure!(
        valid_hash(&json!(revision), 40) && revision != PIN,
        "wrong archived candidate identity"
    );
    ensure!(
        verifier_hash == LEGACY_VERIFIER
            || verifier_hash == "83808058a812e58a8aca6f36a13922c820e546356635ab1ca249c4d33a2e409a"
            || verifier_hash == verifier(),
        "unrecognized native verifier identity"
    );
    let original = dir.join("original/native");
    let candidate = dir.join("candidate");
    original::authenticate(&dir.join("original"))?;
    let a = read_json(&original.join("baseline.json"))?;
    let b = read_json(&candidate.join("baseline.json"))?;
    ensure!(
        a["revision"] == PIN && b["revision"] == revision,
        "unexpected test provenance"
    );
    ensure!(
        a["tree"] == original::TREE && b["tree"] == tree && valid_hash(&json!(tree), 40),
        "archived Git tree differs"
    );
    let left = transcripts::command_rows(&original, &a)?;
    let right = transcripts::command_rows(&candidate, &b)?;
    ensure!(left.len() == 22, "incomplete reference matrix");
    let mut regressions = vec![];
    let mut changes = vec![];
    let mut allowed = vec![];
    for name in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
        let (Some(l), Some(r)) = (left.get(name), right.get(name)) else {
            regressions.push(format!("command membership changed: {name}"));
            continue;
        };
        if l["argv"] != r["argv"] {
            regressions.push(format!("command arguments changed: {name}"));
        }
        if l["status"] != r["status"] || l["exitCode"] != r["exitCode"] {
            changes
                .push(json!({"command":name,"original":l["exitCode"],"candidate":r["exitCode"]}));
            if !matches!(name.as_str(), "installed" | "workspace") || r["exitCode"] != 0 {
                regressions.push(format!("command outcome changed: {name}"));
            }
        }
        if r["timedOut"] == true {
            regressions.push(format!("timeout: {name}"));
        }
        if matches!(
            name.as_str(),
            "cfg" | "cargo" | "clang" | "git" | "python" | "rustc" | "sdk" | "uv"
        ) || name.ends_with("-help")
            || name.ends_with("-version")
        {
            let mut same = l["stderrSha256"] == r["stderrSha256"];
            if name == "sdk" && !same {
                same = transcripts::sdk(&read(&original.join("logs/sdk.stderr"))?)
                    == transcripts::sdk(&read(&candidate.join("logs/sdk.stderr"))?);
                if same {
                    allowed.push(json!({
                        "command": name, "stream": "stderr",
                        "invariant": "Only the generated eight-character suffix of a denied xcrun cache file varies.",
                        "originalSha256": l["stderrSha256"],
                        "candidateSha256": r["stderrSha256"]
                    }));
                }
            }
            if l["stdoutSha256"] != r["stdoutSha256"] || !same || l["exitCode"] != r["exitCode"] {
                regressions.push(format!("public output or tool identity changed: {name}"));
            }
        }
    }
    if a["isolation"]["profileSha256"] != b["isolation"]["profileSha256"]
        || b["isolation"]["profileSha256"] != digest(&read(&candidate.join("baseline-macos.sb"))?)
    {
        regressions.push("native sandbox profile changed".into());
    }
    for key in ["host", "tools", "locks", "flags", "platforms"] {
        if a[key] != b[key] {
            regressions.push(format!("native configuration changed: {key}"));
        }
    }
    for mode in ["metadata", "installed-metadata"] {
        let path = format!("logs/{mode}.stdout");
        if transcripts::metadata(&original.join(&path))?
            != transcripts::metadata(&candidate.join(&path))?
        {
            regressions.push(format!("resolved Cargo metadata changed: {mode}"));
        }
    }
    let path = candidate.join("build-metadata.txt");
    if b["sourceBuild"]["commit"] != revision || !path.is_file() {
        regressions.push("candidate release build metadata missing".into());
    } else {
        let raw = read(&path)?;
        let body = std::str::from_utf8(&raw)?;
        if b["sourceBuild"]["metadataSha256"] != digest(&raw)
            || !body
                .lines()
                .any(|l| l == format!("cargo:rustc-env=MARMISH_BUILD_COMMIT={revision}"))
            || !body
                .lines()
                .any(|l| l == format!("cargo:rustc-env=MARMISH_BUILD_TARGET={TARGET}"))
        {
            regressions.push("candidate release build metadata changed".into());
        }
    }
    let (tests, cohorts, failures) = transcripts::analyze_suites(dir, &mut regressions)?;
    let mut cli = json!({});
    let mut binaries = json!({});
    for binary in BINS {
        for option in ["version", "help"] {
            let name = format!("{binary}-{option}");
            let mut pair = Map::new();
            for (label, rows) in [("original", &left), ("candidate", &right)] {
                if let Some(row) = rows.get(&name) {
                    let projection: Map = ["argv", "exitCode", "stdoutSha256", "stderrSha256"]
                        .into_iter()
                        .map(|k| (k.to_owned(), row[k].clone()))
                        .collect();
                    pair.insert(label.into(), json!(projection));
                }
            }
            cli[&name] = json!(pair);
        }
        let value = &b["binaries"][binary];
        binaries[binary] = json!({"original":a["binaries"][binary],"candidate":value});
        if !valid_hash(&value["sha256"], 64) || value["size"].as_u64().unwrap_or(0) == 0 {
            regressions.push(format!("release binary unobserved: {binary}"));
        }
    }
    let probe = b
        .get("installedStatus")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if candidate.join("install-status.stdout").is_file()
        && candidate.join("install-status.stderr").is_file()
    {
        if installed::check(&candidate, &probe).is_err() {
            regressions.push("installed layout output differs from expected bytes".into());
        }
    } else {
        regressions.push("installed layout transcript missing".into());
    }
    if probe["exitCode"] != 0
        || !["stdoutMatchesExpected", "stderrEmpty", "fixtureUnchanged"]
            .iter()
            .all(|k| probe[k] == true)
        || probe["fixtureBefore"] != probe["fixtureAfter"]
    {
        regressions.push("installed layout observation failed".into());
    }
    let status = if !regressions.is_empty() {
        "regressed"
    } else if !failures.is_empty() {
        "baseline failures remain"
    } else {
        "passed measured observations"
    };
    Ok(json!({
        "schemaVersion":1,"status":status,"originalCommit":a["revision"],"originalTree":a["tree"],
        "candidateCommit":revision,"candidateTree":b["tree"],"host":b["host"],"platforms":b["platforms"],
        "nativeCommands":right.len(),"binaries":binaries,"runnerSha256":b["runnerSha256"],"verifierSha256":verifier_hash,
        "originalArchiveSha256":original::ARCHIVE_HASH,"binaryComparison":"Source builds include distinct commit metadata. Each digest identifies its own artifact.",
        "sourceBuild":b["sourceBuild"],"installStatus":probe,"tests":tests,"cli":cli,
        "historical":read_json(&root().join("examples/marmish/observations.json"))?["historical"],
        "isolatedInstalledDriverUnitTests":cohorts,"currentFailures":failures,"commandDifferences":changes,"regressions":regressions,
        "allowedVariations":allowed,"improvements":transcripts::improvements(&changes,&tests)?,
        "unobserved":["release artifacts and installed multi-host cohorts","native Linux x86-64 and ARM64","interactive Keychain and Tor","remote dispatch"]
    }))
}
#[cfg(test)]
mod tests;
