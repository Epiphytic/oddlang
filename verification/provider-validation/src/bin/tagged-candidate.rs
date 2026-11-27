use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use clap::Parser;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
struct Args {
    oddlang: PathBuf,
    upstream: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pins {
    revision: String,
    contract: PathBuf,
    contract_sha256: String,
    overlay: Overlay,
    sources: BTreeMap<String, Source>,
}

#[derive(Deserialize)]
struct Overlay {
    file: PathBuf,
    sha256: String,
    variant: String,
}

#[derive(Deserialize)]
struct Source {
    path: PathBuf,
    sha256: String,
}

fn require(ok: bool, message: &str) -> Result<()> {
    if !ok {
        return Err(message.into());
    }
    Ok(())
}

fn successful(result: Output) -> Result<Vec<u8>> {
    if !result.status.success() {
        return Err(format!(
            "{}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result.stdout)
}

fn replace_once(source: &str, old: &str, new: &str) -> Result<String> {
    require(
        source.matches(old).count() == 1,
        &format!("expected one mutation anchor {old:?}"),
    )?;
    Ok(source.replacen(old, new, 1))
}

struct Recovery {
    cli: PathBuf,
    selection: PathBuf,
    overlay: PathBuf,
    variant: String,
}

impl Recovery {
    fn digest(&self, graph: &Path) -> Result<String> {
        let out = Command::new(&self.cli)
            .args(["tagged-wire", "digest"])
            .arg(graph)
            .output()?;
        Ok(String::from_utf8(successful(out)?)?.trim().to_owned())
    }

    fn extract(&self, source: &Path, graph: Option<&Path>) -> Result<Output> {
        let mut cmd = Command::new(&self.cli);
        cmd.arg("recover");
        if let Some(graph) = graph {
            cmd.arg("tagged-contract")
                .arg(source)
                .arg(&self.selection)
                .arg(&self.overlay)
                .arg(&self.variant)
                .arg(graph);
        } else {
            cmd.arg("tagged-variant").arg(source).arg(&self.selection);
        }
        Ok(cmd.output()?)
    }

    fn observations(&self, source: &Path) -> Result<Value> {
        Ok(serde_json::from_slice(&successful(
            self.extract(source, None)?,
        )?)?)
    }

    fn materialize(&self, source: &Path, graph: PathBuf) -> Result<String> {
        successful(self.extract(source, Some(&graph))?)?;
        self.digest(&graph)
    }
}

fn check_tags(report: &Value, tag: u64) -> Result<()> {
    require(
        ["encodedTag", "decodedTag"]
            .iter()
            .all(|key| report[key]["value"].as_u64() == Some(tag)),
        "observed tags changed",
    )
}

fn main() -> Result<()> {
    let args = Args::parse();
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/marmish/portable-contract");
    let pins: Pins = serde_json::from_slice(&fs::read(root.join("provenance.json"))?)?;
    let upstream = args.upstream.canonicalize()?;
    let revision = successful(
        Command::new("git")
            .arg("-C")
            .arg(&upstream)
            .args(["rev-parse", "HEAD"])
            .output()?,
    )?;
    require(
        String::from_utf8(revision)?.trim() == pins.revision,
        "upstream revision does not match the pin",
    )?;
    let verify_file = |path: &Path, pin: &str| -> Result<()> {
        require(
            format!("{:x}", Sha256::digest(fs::read(path)?)) == pin,
            &format!("digest changed: {}", path.display()),
        )
    };
    for source in pins.sources.values() {
        verify_file(&upstream.join(&source.path), &source.sha256)?;
    }
    let recovery = Recovery {
        cli: args.oddlang.canonicalize()?,
        selection: root.join("selection.json"),
        overlay: root.join(pins.overlay.file),
        variant: pins.overlay.variant,
    };
    verify_file(&recovery.overlay, &pins.overlay.sha256)?;
    let graph = root.join(&pins.contract);
    let digest = recovery.digest(&graph)?;
    require(digest == pins.contract_sha256, "contract digest changed")?;
    let baseline = recovery.observations(&upstream)?;
    let expected: Value = serde_json::from_slice(&fs::read(root.join("candidate.json"))?)?;
    require(baseline == expected, "baseline changed")?;
    let fields = json!([
        {"name":"offset","position":0,"kind":"uint-be","width":8,"writeLine":396,"writerDefinitionLine":751},
        {"name":"bytes","position":1,"kind":"rest-bytes","writeLine":397}
    ]);
    require(baseline["fields"] == fields, "field order or shape changed")?;
    check_tags(&baseline, 5)?;

    let temp = tempfile::tempdir()?;
    let work = temp.path();
    require(
        recovery.materialize(&upstream, work.join("original.ttl"))? == digest,
        "overlay did not reproduce the pinned graph",
    )?;
    let selection: Value = serde_json::from_slice(&fs::read(&recovery.selection)?)?;
    let selected = selection["file"].as_str().ok_or("missing selected file")?;
    let original = fs::read_to_string(upstream.join(selected))?;
    let copy = work.join(selected);
    fs::create_dir_all(copy.parent().ok_or("missing parent")?)?;
    let encoder = replace_once(
        &original,
        "payload.extend(bytes);\n                5\n",
        "payload.extend(bytes);\n                51\n",
    )?;
    fs::write(&copy, &encoder)?;
    let rejected = recovery.extract(work, None)?;
    require(
        !rejected.status.success()
            && String::from_utf8_lossy(&rejected.stderr).contains("tags disagree")
            && rejected.stdout.is_empty(),
        "conflicting candidate was not diagnosed cleanly",
    )?;
    let output = work.join("rejected.ttl");
    require(
        !recovery.extract(work, Some(&output))?.status.success() && !output.try_exists()?,
        "conflicting graph was emitted",
    )?;

    let both = replace_once(
        &encoder,
        "5 => {\n                let offset = r.offset()?;",
        "51 => {\n                let offset = r.offset()?;",
    )?;
    fs::write(&copy, both)?;
    let changed = recovery.observations(work)?;
    check_tags(&changed, 51)?;
    require(changed["fields"] == fields, "unrelated fields changed")?;
    let expected_graph = work.join("expected-retagged.ttl");
    fs::write(
        &expected_graph,
        replace_once(&fs::read_to_string(graph)?, "t:tag 5 ;", "t:tag 51 ;")?,
    )?;
    let retagged = recovery.materialize(work, work.join("retagged.ttl"))?;
    require(
        retagged != digest && retagged == recovery.digest(&expected_graph)?,
        "materialized graph did not change exactly one tag",
    )?;

    fs::write(copy, original.replace("put_u64", "put_be_integer"))?;
    let renamed = recovery.observations(work)?;
    check_tags(&renamed, 5)?;
    require(
        renamed["sourceSha256"] != baseline["sourceSha256"] && renamed["fields"] == fields,
        "helper rename changed observations or retained source pin",
    )?;
    require(
        recovery.materialize(work, work.join("refactored.ttl"))? == digest,
        "helper rename changed the graph",
    )?;
    println!("Contract pin verified at {digest}.");
    println!("Pinned tag 5 and two payload writes reproduced.");
    println!("Tag 51 changed only the graph tag, yielding {retagged}.");
    println!("A one-sided tag change was rejected without writing a graph.");
    println!("Equivalent integer-writer rename preserved the observations and graph pin.");
    Ok(())
}
