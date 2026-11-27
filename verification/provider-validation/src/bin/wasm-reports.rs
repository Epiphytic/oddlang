//! Summarize two independent Wasm builds and the observations of their consumers.
use provider_validation::{Result, read, sha};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
struct Comparison {
    first: String,
    second: String,
    identical: bool,
}

fn collect(root: &Path, dir: &Path, entries: &mut BTreeMap<PathBuf, String>) -> Result {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(root, &path, entries)?;
        } else if path.is_file()
            && matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "wasm")
            )
        {
            entries.insert(path.strip_prefix(root)?.to_path_buf(), sha(&path)?);
        }
    }
    Ok(())
}

fn guest_hashes(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut entries = BTreeMap::new();
    collect(root, root, &mut entries)?;
    Ok(entries)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Host<'a> {
    case_count: usize,
    host: &'a str,
    version: &'a str,
    component_sha256: &'a str,
    release_sha256: &'a str,
    jco_version: &'a str,
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("expected {key} string").into())
}

fn list<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value[key]
        .as_array()
        .ok_or_else(|| format!("expected {key} array").into())
}

#[derive(Serialize)]
struct Observation<'a> {
    case: &'a str,
    call: &'a Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Imports<'a> {
    case: &'a str,
    observed_functions: Vec<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary<'a> {
    hosts: BTreeMap<String, Host<'a>>,
    builds: BTreeMap<&'a PathBuf, &'a Comparison>,
    trap: Observation<'a>,
    double_import: Imports<'a>,
    upper_endpoint: Observation<'a>,
}

fn summarize(root: &Path, output: &Path) -> Result {
    let first = guest_hashes(&root.join("first/guests"))?;
    let second = guest_hashes(&root.join("second/guests"))?;
    if first.is_empty() || first.keys().ne(second.keys()) {
        return Err("guest outputs differ between clean builds".into());
    }
    let report: BTreeMap<_, _> = first
        .into_iter()
        .map(|(file, first)| {
            let second = second[&file].clone();
            (
                file,
                Comparison {
                    identical: first == second,
                    first,
                    second,
                },
            )
        })
        .collect();
    fs::write(
        output.join("reproducibility.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if report
        .iter()
        .any(|(p, c)| p.extension().is_some_and(|ext| ext == "rs") && !c.identical)
    {
        return Err("generated Rust differs between clean builds".into());
    }

    let mut receipts = BTreeMap::new();
    for entry in fs::read_dir(root.join("hosts"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("invalid host receipt filename")?
                .to_string();
            receipts.insert(name, read(&path)?);
        }
    }
    let mut hosts = BTreeMap::new();
    for (name, row) in &receipts {
        hosts.insert(
            name.clone(),
            Host {
                case_count: list(row, "cases")?.len(),
                host: string(row, "host")?,
                version: string(row, "version")?,
                component_sha256: string(row, "componentSha256")?,
                release_sha256: string(row, "releaseSha256")?,
                jco_version: string(row, "jcoVersion")?,
            },
        );
    }
    let native = |file: &str| read(&root.join("first/wasmtime-reports").join(file));
    let traps = native("trap.json")?;
    let mut trapped = None;
    for case in list(&traps, "cases")? {
        if let Some(call) = list(case, "calls")?
            .iter()
            .find(|c| c.get("trap").is_some())
        {
            trapped = Some(Observation {
                case: string(case, "caseId")?,
                call,
            });
            break;
        }
    }
    let repeated = native("double-import.json")?;
    let case = list(&repeated, "cases")?
        .first()
        .ok_or("missing repeated import case")?;
    let bounds = native("max-exclusive.json")?;
    let boundary = list(&bounds, "cases")?
        .iter()
        .find(|c| c["caseId"] == "exact-destination-headroom")
        .ok_or("missing upper endpoint case")?;
    let summary = Summary {
        hosts,
        builds: report
            .iter()
            .filter(|(p, _)| p.extension().is_some_and(|ext| ext == "wasm"))
            .collect(),
        trap: trapped.ok_or("missing observed trap")?,
        double_import: Imports {
            case: string(case, "caseId")?,
            observed_functions: list(case, "imports")?
                .iter()
                .map(|v| string(v, "function"))
                .collect::<Result<_>>()?,
        },
        upper_endpoint: Observation {
            case: string(boundary, "caseId")?,
            call: list(boundary, "calls")?
                .first()
                .ok_or("missing endpoint call")?,
        },
    };
    fs::write(
        output.join("runtime-observations.json"),
        format!("{}\n", serde_json::to_string_pretty(&summary)?),
    )?;
    Ok(())
}
fn main() -> Result {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [root] => summarize(root.as_ref(), root.as_ref()),
        [root, out] => summarize(root.as_ref(), out.as_ref()),
        _ => Err("usage: wasm-reports VERIFICATION_ROOT [REPORT_DIRECTORY]".into()),
    }
}
