use super::*;
use serde::Serialize;
use std::fmt::Write;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub role: String,
    pub root: PathBuf,
    baseline_sha256: String,
    cargo_inputs: Value,
    sources: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bundle_sha256: Option<String>,
}
// The frozen cross-language binding uses sorted JSON keys, ASCII escaping
// and a space after each comma and colon.
fn string(input: &str) -> String {
    let quoted = serde_json::to_string(input).expect("JSON string");
    let mut out = String::new();
    for c in quoted.chars() {
        if ('\u{7f}'..).contains(&c) {
            for unit in c.encode_utf16(&mut [0; 2]).iter() {
                write!(out, "\\u{unit:04x}").unwrap();
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn canonical(v: &Value) -> String {
    match v {
        Value::String(s) => string(s),
        Value::Array(xs) => format!(
            "[{}]",
            xs.iter().map(canonical).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(xs) => {
            let mut entries = xs.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(k, _)| *k);
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(k, v)| format!("{}: {}", string(k), canonical(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        _ => v.to_string(),
    }
}
pub fn binding(v: &Value) -> String {
    digest(canonical(v).as_bytes())
}
fn sources(root: &Path, path: &Path, out: &mut Map) -> Result<()> {
    for item in fs::read_dir(path)? {
        let item = item?;
        let p = item.path();
        let kind = item.file_type()?;
        if kind.is_dir() {
            sources(root, &p, out)?;
        } else if p.extension().is_some_and(|ext| ext == "rs") {
            ensure!(
                kind.is_file(),
                "driver workspace source links to an unverified file"
            );
            out.insert(
                p.strip_prefix(root)?
                    .to_str()
                    .context("invalid source path")?
                    .into(),
                json!(sha(&p)?),
            );
        }
    }
    Ok(())
}
pub fn identify(args: &Args, root: &Path, upstream: &str) -> Result<Workspace> {
    let freeze = read_json(&args.baseline)?;
    ensure!(
        freeze["upstreamCommit"] == upstream,
        "workspace baseline pins an unexpected upstream"
    );
    let mut cargo = Map::new();
    for p in object(&freeze["cargoInputs"])?.keys() {
        ensure!(
            Path::new(p)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "invalid Cargo input path"
        );
        cargo.insert(p.clone(), json!(sha(&root.join(p))?));
    }
    ensure!(
        freeze["cargoInputs"] == json!(cargo),
        "driver workspace manifests or lock differ from the freeze"
    );
    let mut observed = Map::new();
    sources(root, &root.join("crates/marmish-stream"), &mut observed)?;
    let expected: Map = object(&freeze["trackedSources"])?
        .iter()
        .map(|(k, v)| (k.clone(), v["sha256"].clone()))
        .collect();
    ensure!(
        observed.keys().eq(expected.keys()),
        "driver workspace stream Rust source set differs from the freeze"
    );
    ensure!(
        (observed != expected) == (args.role == "mutation"),
        "driver source bytes disagree with the requested build role"
    );
    let commit = if let Some(archive) = &args.archive_sha256 {
        ensure!(
            valid_hash(&json!(archive), 64) && valid_hash(&json!(args.commit), 40),
            "archive provenance needs a full commit and SHA-256"
        );
        args.commit.clone()
    } else if args.role == "original" {
        let call = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()?;
        ensure!(call.status.success(), "git identity unavailable");
        Some(String::from_utf8(call.stdout)?.trim().to_owned())
    } else {
        None
    };
    if args.role == "original" {
        ensure!(
            commit.as_deref() == Some(upstream),
            "original driver must execute from the pinned upstream"
        );
    }
    let bundle = if args.role == "candidate" {
        ensure!(
            args.archive_sha256.is_some()
                && commit.as_deref() != Some(upstream)
                && args.bundle_sha256.is_some(),
            "candidate driver requires qualified downstream archive and bundle pins"
        );
        let actual = sha(&root.join("oddlang/stream/bundle.lock"))?;
        ensure!(
            args.bundle_sha256.as_ref() == Some(&actual),
            "candidate driver bundle pin disagrees with the staged checkout"
        );
        Some(actual)
    } else {
        None
    };
    Ok(Workspace {
        role: args.role.clone(),
        root: root.into(),
        baseline_sha256: sha(&args.baseline)?,
        cargo_inputs: json!(cargo),
        sources: json!(observed),
        commit,
        archive_sha256: args.archive_sha256.clone(),
        bundle_sha256: bundle,
    })
}
