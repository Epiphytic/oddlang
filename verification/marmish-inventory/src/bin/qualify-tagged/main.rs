//! Compare generated local Data encoders and incremental readers with the pinned original.
mod build;
#[path = "../portable-oracle/corpus.rs"]
mod corpus;
mod linux;
#[path = "../portable-oracle/native.rs"]
mod native;
mod protocol;
mod sandbox;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use marmish_inventory::{array, read, read_json, root, strings, text};
use native::{checked, sum};
use sandbox::{Denied, Tools};
use serde_json::{Value as V, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::Command,
};
const PIN: &str = "82ddfed871221091be6b42adf6d0b009cb5ce10f864c414bdbaed5e19e4dad56";
#[derive(Parser)]
struct Args {
    #[arg(long)]
    oddlang: PathBuf,
    #[arg(long)]
    upstream: PathBuf,
    #[arg(long)]
    graph: Option<PathBuf>,
    #[arg(long)]
    hints: Option<PathBuf>,
    #[arg(long)]
    holdout: Option<PathBuf>,
    #[arg(long)]
    deny_read: Vec<PathBuf>,
    #[arg(long)]
    receipt: PathBuf,
    #[arg(long)]
    external_judge: bool,
    #[arg(long)]
    linux_privileged_mounts: bool,
    #[command(flatten)]
    original: native::Original,
}
fn select(cases: Vec<V>) -> Result<Vec<V>> {
    let mut rows = vec![];
    for c in cases {
        if protocol::comparable(&c)? {
            rows.push(c);
        }
    }
    Ok(rows)
}
fn main() -> Result<()> {
    let a = Args::parse();
    let tools = Tools::detect(a.linux_privileged_mounts)?;
    let dir = corpus::directory();
    let contract = dir.join("../portable-contract");
    let input = a.graph.unwrap_or_else(|| contract.join("data.ttl"));
    let data = String::from_utf8(read(&input)?)?;
    ensure!(
        data.matches("t:tag 5 ;").count() == 1,
        "graph must have exactly one Data tag"
    );
    let changed = data.replace("t:tag 5 ;", "t:tag 51 ;");
    let corpus = corpus::public()?;
    let corpus_counts =
        json!({"prior":corpus.prior,"authored":corpus.authored,"seeded":corpus.seeded});
    let mut public = select(corpus.cases)?;
    public.extend(array(&read_json(&dir.join("tagged-supplement.json"))?)?.clone());
    let sealed = if let Some(p) = &a.holdout {
        select(corpus::sealed(p, &corpus.freeze)?)?
    } else {
        vec![]
    };
    let upstream = a.upstream.canonicalize()?;
    let oddlang = a.oddlang.canonicalize()?;
    let temp = tempfile::tempdir()?;
    let work = temp.path().canonicalize()?;
    let withheld = work.join("withheld");
    fs::create_dir(&withheld)?;
    let sentinel = withheld.join("structure.ttl");
    fs::write(
        &sentinel,
        "<urn:withheld:source> <urn:withheld:edge> <urn:withheld:callee> .\n",
    )?;
    let mut denied: Denied = [
        (
            "upstreamSource".into(),
            upstream.join("crates/marmish-stream/src/copy_protocol.rs"),
        ),
        (
            "oracleVectors".into(),
            dir.join("../stream-vectors.json").canonicalize()?,
        ),
        ("withheldStructureProbe".into(), sentinel),
    ]
    .into_iter()
    .collect();
    let mut extra = json!({});
    for (i, p) in a.deny_read.iter().enumerate() {
        let p = p.canonicalize()?;
        let id = format!("withheldInput{i}");
        let name = p.file_name().context("withheld input")?.to_string_lossy();
        extra[&id] = json!({"role":if name=="structure.ttl"{"privateStructureGraph"}else{"withheldInput"},"file":name,"sha256":sum(&p)?});
        denied.insert(id, p);
    }
    if let Some(p) = &a.holdout {
        denied.insert("sealedCapsule".into(), p.canonicalize()?);
    }
    let hint_path = contract.join("hints.json");
    let hint = a
        .hints
        .or_else(|| hint_path.is_file().then(|| hint_path.clone()));
    let mut hint_evidence = V::Null;
    let copy = withheld.join("advisory-hints");
    if let Some(path) = hint {
        let p = path.canonicalize()?;
        fs::copy(&p, &copy)?;
        let name = p.file_name().context("hint filename")?.to_string_lossy();
        let hash = sum(&copy)?;
        if hint_path.is_file() && p == hint_path.canonicalize()? {
            ensure!(
                fs::read_to_string(hint_path.with_extension("sha256"))?
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    == [hash.as_str(), name.as_ref()],
                "hint digest mismatch"
            );
        }
        hint_evidence = json!({"file":name,"sha256":hash,"withheldOnFirstBuild":true,"consumedByBuilder":false,"withheldCopyPresenceOrAbsencePreservedOutput":true});
        denied.insert("advisoryHintSource".into(), p);
        denied.insert("withheldHintCopy".into(), copy.clone());
    }
    let rel = work.join("release");
    let alt = work.join("changed-tag");
    let nohint = work.join("no-hints");
    let mut original = build::project(&rel, &oddlang, data.as_bytes(), true, &denied, &tools)?;
    if copy.exists() {
        fs::remove_file(&copy)?;
        denied.remove("withheldHintCopy");
    }
    let mut mutant = build::project(&alt, &oddlang, changed.as_bytes(), false, &denied, &tools)?;
    let mut omitted = build::project(&nohint, &oddlang, data.as_bytes(), true, &denied, &tools)?;
    ensure!(
        original["graphSha256"] == omitted["graphSha256"]
            && original["generatedSourceSha256"] == omitted["generatedSourceSha256"]
            && original["graphSha256"] != mutant["graphSha256"],
        "graph control failure"
    );
    let adapters = build::compile(&rel, &tools, &denied, &mut original)?;
    let mutants = build::compile(&alt, &tools, &denied, &mut mutant)?;
    let restored = build::compile(&nohint, &tools, &denied, &mut omitted)?;
    if a.external_judge {
        let requests = strings(&serde_json::from_reader(io::stdin().lock())?)?;
        let mut result = json!({});
        for (n, cmd) in &adapters {
            result[n] = json!(protocol::rows(&rel, cmd, &requests)?);
        }
        println!("{}", serde_json::to_string(&result)?);
        io::stdout().flush()?;
    }
    let mut compared = json!({});
    for (n, cmd) in &adapters {
        compared[n] = json!(protocol::assert_rows(&rel, cmd, &public)?.len());
        if !sealed.is_empty() {
            ensure!(
                protocol::assert_rows(&rel, cmd, &sealed).is_ok(),
                "sealed recovered target replay failed"
            );
        }
    }
    let w = public
        .iter()
        .find(|r| r["name"] == "binary-data-encode")
        .context("tag witness")?;
    let mutations = protocol::mutation(&rel, &alt, &adapters, &mutants, w)?;
    let (cmd, provenance) = native::build(&upstream, &work.join("upstream-build"), &a.original)?;
    let binding = text(&provenance["callerBinding"])?;
    protocol::original(&cmd, &public, binding)?;
    if !sealed.is_empty() {
        let mut bound = native::holdout_boundary(a.holdout.as_ref().unwrap())?;
        bound.extend(cmd.clone());
        ensure!(
            protocol::original(&bound, &sealed, binding).is_ok(),
            "sealed original replay failed"
        );
    }
    protocol::original(&cmd, &mutations, binding)?;
    let mixed = protocol::peers(&rel, &adapters, &public, &cmd, binding)?;
    for cmd in restored.values() {
        protocol::assert_rows(&nohint, cmd, &public)?;
    }
    let mut counts = BTreeMap::new();
    for c in &public {
        *counts.entry(text(&c["operation"])?).or_insert(0) += 1;
    }
    let names = ["rust", "typescript"];
    let outputs: BTreeMap<_, _> = names
        .iter()
        .zip(&mutations)
        .map(|(n, c)| (n, &c["input"]))
        .collect();
    let mut sources = json!({});
    for f in [
        "qualify-tagged/main.rs",
        "qualify-tagged/build.rs",
        "qualify-tagged/protocol.rs",
        "qualify-tagged/sandbox.rs",
        "qualify-tagged/linux.rs",
        "portable-oracle/corpus.rs",
        "portable-oracle/native.rs",
    ] {
        sources[f] = json!(sum(&root()
            .join("verification/marmish-inventory/src/bin")
            .join(f))?);
    }
    for f in [
        "authored-cases.json",
        "tagged-supplement.json",
        "tagged_adapter.rs",
        "tagged_adapter.mjs",
    ] {
        sources[f] = json!(sum(&dir.join(f))?);
    }
    let mut runtime = tools.platform.clone();
    runtime.as_object_mut().unwrap().extend(
        json!({
            "host":std::env::consts::OS,
            "rust":checked(Command::new(&tools.rustc).arg("-vV"),20)?.lines().collect::<Vec<_>>(),
            "rustcSha256":sum(&tools.rustc)?,
            "clangSha256":sum(&tools.clang)?,
            "node":checked(Command::new(&tools.node).arg("--version"),20)?.trim(),
            "nodeSha256":sum(&tools.node)?,
            "nodeOptions":["--no-warnings","--experimental-strip-types"],
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    let receipt = json!({
        "schema":1,
        "contract":"urn:marmish:portable:copy-frame:v1:Data",
        "approvedGraphSha256":PIN,
        "additionalWithheldInputs":extra,
        "upstreamRevision":provenance["revision"],
        "corpusRevision":corpus.freeze["revision"],
        "freezeSha256":sum(&dir.join("freeze.json"))?,
        "sourceCorpusCounts":corpus_counts,
        "publicCases":counts,
        "publicCount":public.len(),
        "publicCaseIds":public.iter().map(|r|&r["name"]).collect::<Vec<_>>(),
        "sealedCases":sealed.len(),
        "comparedOperationResults":compared,
        "driverSourceSha256":sources,
        "mixedPeers":mixed,
        "build":original,
        "hintOmission":hint_evidence,
        "alternativeLibraryValidation":"unresolved",
        "buildWithoutHints":omitted,
        "mutatedBuild":mutant,
        "mutatedOutputs":outputs,
        "mutation":{"from":5,"to":51,"upstreamRejected":names,"mutantsDecodedOwnWire":names,"mutantsFedOwnWire":names,"mutantsRejectedOriginalDecodeAndFeed":names,"wrongEncodeTagsRejected":names,"restoredPinReplayed":true},
        "originalCaller":provenance,
        "runtime":runtime,
        "gaps":["Copy variants other than Data and mixed frames requiring another variant","Windows, codec, stream JSON wire, bridge command and host effects","Full Rust public paths, signatures, traits, bindings and native consumers","Invalid portable values and trailing payload are outside the selected upstream comparison","Successful reads confined; file metadata probes remain allowed","Runtime dependencies and platforms beyond those recorded","Alternative compression library role is outside the Data contract"],
    });
    fs::write(a.receipt, serde_json::to_string_pretty(&receipt)? + "\n")?;
    let msg = format!(
        "{} public cases; {} sealed cases; both native targets and graph tag mutation passed",
        public.len(),
        sealed.len()
    );
    if a.external_judge {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
    Ok(())
}
