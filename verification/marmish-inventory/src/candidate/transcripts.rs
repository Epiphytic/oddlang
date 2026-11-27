use super::*;
use regex::Regex;
use std::sync::LazyLock;
pub type Outcomes = BTreeMap<String, String>;
pub fn outcomes(stdout: &str, stderr: &str) -> Result<Outcomes> {
    static RUN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*Running .+? \((.+)\)$").unwrap());
    static DOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*Doc-tests (\S+)$").unwrap());
    static SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-[0-9a-f]{16}$").unwrap());
    static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; 0 filtered out;.*$").unwrap()
    });
    static TEST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^test (.+?) \.\.\. (ok|FAILED|ignored(?:, .*)?|)$").unwrap());
    let mut targets = vec![];
    for line in stderr.lines() {
        if let Some(m) = RUN.captures(line) {
            targets.push(
                SUFFIX
                    .replace(m[1].rsplit('/').next().unwrap(), "")
                    .into_owned(),
            );
        } else if let Some(m) = DOC.captures(line) {
            targets.push(format!("doc/{}", &m[1]));
        }
    }
    let summaries: Vec<_> = SUMMARY.captures_iter(stdout).collect();
    ensure!(
        !summaries.is_empty() && summaries.len() == targets.len(),
        "incomplete libtest run or missing target identity"
    );
    let mut out = Outcomes::new();
    let mut start = 0;
    for (target, summary) in targets.iter().zip(summaries) {
        let m = summary.get(0).unwrap();
        let mut pending = None;
        let mut rows = BTreeMap::new();
        for line in stdout[start..m.start()].lines() {
            if let Some(row) = TEST.captures(line) {
                if row[2].is_empty() {
                    pending = Some(row[1].to_owned());
                } else {
                    rows.insert(row[1].to_owned(), row[2].to_owned());
                }
            } else if matches!(line, "ok" | "FAILED")
                && let Some(name) = pending.take()
            {
                rows.insert(name, line.to_owned());
            }
        }
        let counts = counts(&rows);
        for (i, kind) in ["ok", "FAILED", "ignored"].into_iter().enumerate() {
            ensure!(
                counts.get(kind).copied().unwrap_or(0) == summary[i + 1].parse::<usize>()?,
                "individual outcomes disagree with libtest summary for {target}"
            );
        }
        for (name, status) in rows {
            let key = format!("{target}/{name}");
            ensure!(
                out.insert(key.clone(), status).is_none(),
                "duplicate test identity: {key}"
            );
        }
        start = m.end();
    }
    Ok(out)
}
pub fn counts(rows: &Outcomes) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for value in rows.values() {
        *counts
            .entry(value.split(", ").next().unwrap().to_owned())
            .or_default() += 1;
    }
    counts
}
pub fn deltas(old: &Outcomes, new: &Outcomes) -> Vec<Value> {
    let names: BTreeSet<_> = old.keys().chain(new.keys()).collect();
    names
        .into_iter()
        .filter(|name| old.get(*name) != new.get(*name))
        .map(|name| {
            json!({
                "test": name,
                "original": old.get(name).map(String::as_str).unwrap_or("unobserved"),
                "candidate": new.get(name).map(String::as_str).unwrap_or("unobserved"),
            })
        })
        .collect()
}
pub fn improvements(commands: &[Value], tests: &Map) -> Result<Vec<Value>> {
    let mut fixes = vec![];
    for row in commands {
        if matches!(row["command"].as_str(), Some("workspace" | "installed"))
            && row["original"] != 0
            && row["candidate"] == 0
        {
            let mut fixed = row.clone();
            fixed["kind"] = json!("cargo-exit");
            fixes.push(fixed);
        }
    }
    for mode in ["workspace", "installed"] {
        if let Some(test) = tests.get(mode) {
            for row in array(&test["differences"])? {
                if row["candidate"] == "ok"
                    && (row["original"] == "FAILED"
                        || text(&row["original"])?.starts_with("ignored"))
                {
                    let mut fixed = row.clone();
                    fixed["kind"] = json!("test");
                    fixed["suite"] = json!(mode);
                    fixes.push(fixed);
                }
            }
        }
    }
    Ok(fixes)
}
pub fn sdk(bytes: &[u8]) -> Vec<u8> {
    static CACHE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
        regex::bytes::Regex::new(r"(?m)^(xcrun: error: couldn't create cache file '/var/folders/[^/\n']+/[^/\n']+/T/xcrun_db-)[A-Za-z0-9]{8}(' \(errno=Operation not permitted\))$").unwrap()
    });
    CACHE
        .replace_all(bytes, &b"${1}<RANDOM>${2}"[..])
        .into_owned()
}
pub fn metadata(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path)?;
    static DIR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"/private/tmp/m-[^/\\\s"]+/"#).unwrap());
    Ok(serde_json::from_str(
        &DIR.replace_all(&raw, regex::NoExpand("$RUN/")),
    )?)
}
pub fn command_rows(dir: &Path, state: &Value) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for row in array(&state["commands"])? {
        let name = text(&row["name"])?;
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !out.contains_key(name),
            "invalid native command name"
        );
        for stream in ["stdout", "stderr"] {
            let path = format!("logs/{name}.{stream}");
            ensure!(
                row[stream] == path
                    && row[format!("{stream}Sha256")] == digest(&read(&dir.join(path))?),
                "native stream digest mismatch: {name}.{stream}"
            );
        }
        let status = if row["timedOut"].as_bool().context("missing timedOut")? {
            "timed-out"
        } else if row["exitCode"] != 0 {
            "failed"
        } else {
            "passed"
        };
        ensure!(row["status"] == status, "invalid native status: {name}");
        out.insert(name.to_owned(), row.clone());
    }
    Ok(out)
}
pub fn analyze_suites(dir: &Path, regressions: &mut Vec<String>) -> Result<(Map, Value, Set)> {
    let mut tests = Map::new();
    let mut cohorts = json!({});
    let mut failures = Set::new();
    for mode in ["workspace", "installed"] {
        let mut evidence = json!({});
        let mut outcomes_by_role = Vec::new();
        let mut logs = Vec::new();
        for (label, folder) in [
            ("original", "original/native/logs"),
            ("candidate", "candidate/logs"),
        ] {
            let path = format!("{folder}/{mode}.stdout");
            let stdout = fs::read_to_string(dir.join(&path))?;
            let stderr = fs::read_to_string(dir.join(folder).join(format!("{mode}.stderr")))?;
            let rows = outcomes(&stdout, &stderr)?;
            for (name, result) in &rows {
                if label == "candidate" && result == "FAILED" {
                    failures.insert(name.clone());
                }
                if mode == "installed"
                    && let Some((_, suffix)) = name.split_once("::installed_check::")
                {
                    cohorts[suffix.split("::").next().unwrap()][label][name] = json!(result);
                }
            }
            let list = fs::read_to_string(dir.join(folder).join(format!("{mode}-ignored.stdout")))?;
            let mut ignored: Vec<_> = list
                .lines()
                .filter_map(|s| s.strip_suffix(": test"))
                .collect();
            ignored.sort();
            evidence["ignoredLists"][label] = json!(ignored);
            evidence["counts"][label] = json!(counts(&rows));
            evidence[label] = json!(rows);
            outcomes_by_role.push(rows);
            logs.push((label, path, stdout));
        }
        let mut differences = deltas(&outcomes_by_role[0], &outcomes_by_role[1]);
        for diff in &mut differences {
            let name = text(&diff["test"])?.to_owned();
            let needle = format!(
                "---- {} stdout ----",
                name.split_once('/').context("invalid test ID")?.1
            );
            for (label, path, body) in &logs {
                let locations: Vec<_> = body
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| line == &needle)
                    .map(|(i, _)| i + 1)
                    .collect();
                diff["transcripts"][label] = json!({"path":path,"failureLines":locations});
            }
            if diff["candidate"] != "ok" || diff["original"] == "unobserved" {
                regressions.push(format!(
                    "{mode}: {name} ({} -> {})",
                    text(&diff["original"])?,
                    text(&diff["candidate"])?
                ));
            }
        }
        if evidence["ignoredLists"]["original"] != evidence["ignoredLists"]["candidate"] {
            regressions.push(format!("ignored test list changed: {mode}"));
        }
        evidence["differences"] = json!(differences);
        tests.insert(mode.into(), evidence);
    }
    Ok((tests, cohorts, failures))
}
