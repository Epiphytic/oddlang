mod corpus;
mod native;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Parser;
use corpus::{directory, hash};
use flate2::{
    Compression,
    write::{GzEncoder, ZlibEncoder},
};
use marmish_inventory::{
    array, digest, hex_decode, hex_encode, root,
    stream_observer::{expand, matches},
    text,
};
use native::{observe, sum};
use serde_json::{Value as V, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Parser)]
struct Args {
    #[arg(long, required_unless_present = "command", conflicts_with = "command")]
    upstream: Option<PathBuf>,
    #[arg(long,num_args=1..)]
    command: Vec<String>,
    #[arg(long,num_args=1..)]
    peer_command: Vec<String>,
    #[arg(long)]
    holdout: Option<PathBuf>,
    #[arg(long)]
    receipt: PathBuf,
    #[arg(long)]
    observations: Option<PathBuf>,
    #[command(flatten)]
    original: native::Original,
}
fn fails(cases: &[V], obs: &[V]) -> Result<Vec<String>> {
    ensure!(obs.len() == cases.len(), "observation length mismatch");
    let mut failures = vec![];
    for (c, r) in cases.iter().zip(obs) {
        ensure!(c["name"] == r["name"], "observation identity mismatch");
        let expected = expand(&c["expected"])?;
        let actual = &r["actual"];
        let fields = expected.get("zlib_roundtrip_hex").is_none()
            || actual.as_object().is_some_and(|o| {
                o.len() == 5
                    && ["codec", "offset", "raw_len", "data", "sha256"]
                        .iter()
                        .all(|k| o.contains_key(*k))
            });
        if !fields || !matches(actual, &expected) {
            failures.push(text(&c["name"])?.to_owned());
        }
    }
    Ok(failures)
}
fn controls(cases: &[V], obs: &[V]) -> Result<V> {
    let mut result = json!({});
    for (label, name) in [
        ("copy-data-tag", "binary-data-encode"),
        ("error-precedence", "codec-block-length-before-offset"),
        ("rejected-transition", "seed-0-receive-retry"),
    ] {
        let mut rows = cases.to_vec();
        let e = &mut rows
            .iter_mut()
            .find(|c| c["name"] == name)
            .context("control case")?["expected"];
        match label {
            "copy-data-tag" => *e = json!(format!("0c{}", &text(e)?[2..])),
            "error-precedence" => *e = json!({"error":"Offset"}),
            _ => e[7]["delivered"] = json!(e[7]["delivered"].as_u64().context("delivered")? + 1),
        }
        let rejected = fails(&rows, obs)?;
        ensure!(rejected == [name], "control {label} failed");
        result[label] = json!(rejected);
    }
    Ok(result)
}
fn mixed(
    cases: &[V],
    writer: &[String],
    reader: &[String],
    wb: Option<&str>,
    rb: Option<&str>,
) -> Result<V> {
    let index: BTreeMap<_, _> = cases
        .iter()
        .map(|c| Ok((text(&c["name"])?, c)))
        .collect::<Result<_>>()?;
    let mut writes = vec![];
    let mut reads = vec![];
    for c in cases {
        if !["frame_encode", "request_encode", "response_encode"].contains(&text(&c["operation"])?)
        {
            continue;
        }
        if let Some(id) = text(&c["name"])?.strip_suffix("-encode")
            && let Some(read) = index.get(format!("{id}-decode").as_str())
        {
            writes.push(c.clone());
            reads.push((*read).clone());
        }
    }
    let out = observe(writer, &writes, wb)?;
    ensure!(fails(&writes, &out)?.is_empty(), "mixed writer mismatch");
    for (r, o) in reads.iter_mut().zip(out) {
        r["input"] = o["actual"].clone();
    }
    let out = observe(reader, &reads, rb)?;
    ensure!(fails(&reads, &out)?.is_empty(), "mixed reader mismatch");
    Ok(json!(out.iter().map(|r| &r["name"]).collect::<Vec<_>>()))
}
#[link(name = "z")]
unsafe extern "C" {
    fn zlibVersion() -> *const std::ffi::c_char;
}

fn zlib(cases: &[V], obs: &[V], cmd: &[String], binding: Option<&str>) -> Result<V> {
    let i = cases
        .iter()
        .position(|c| c["name"] == "codec-zlib-encode")
        .context("zlib case")?;
    let mut shape = obs[i]["actual"].clone();
    let expected = expand(&cases[i]["expected"])?;
    let raw = hex_decode(text(&expected["zlib_roundtrip_hex"])?)?;
    let written = STANDARD.decode(text(&shape["data"])?)?;
    ensure!(
        matches(&shape, &expected),
        "system zlib rejected Rust bytes"
    );
    let mut alternate = None;
    for level in [1, 9] {
        let mut z = ZlibEncoder::new(vec![], Compression::new(level));
        z.write_all(&raw)?;
        let bytes = z.finish()?;
        if bytes != written && bytes.len() < raw.len() {
            alternate = Some(bytes);
            break;
        }
    }
    let bytes = alternate.context("no distinct system zlib encoding")?;
    shape["data"] = json!(STANDARD.encode(&bytes));
    let reads = [corpus::case(
        "independent-zlib-write",
        "codec_decode",
        json!({"block":shape,"negotiated":"zlib"}),
        json!(hex_encode(&raw)),
    )];
    ensure!(
        fails(&reads, &observe(cmd, &reads, binding)?)?.is_empty(),
        "caller rejected system zlib bytes"
    );
    // The system library returns a static, NUL-terminated version string.
    let version = unsafe { std::ffi::CStr::from_ptr(zlibVersion()) }.to_str()?;
    Ok(
        json!({"systemZlibVersion":version,"rawSha256":digest(&raw),"rawBytes":raw.len(),"rustZlibHex":hex_encode(&written),"independentZlibHex":hex_encode(&bytes),"directions":["Marmish encoder to system zlib decoder","system zlib encoder to Marmish decoder"]}),
    )
}
fn main() -> Result<()> {
    let a = Args::parse();
    let public = corpus::public()?;
    let cases = &public.cases;
    let sealed = a
        .holdout
        .as_deref()
        .map(|p| corpus::sealed(p, &public.freeze))
        .transpose()?;
    let temp = tempfile::tempdir()?;
    let (cmd, provenance) = if let Some(work) = a.upstream {
        native::build(&work.canonicalize()?, temp.path(), &a.original)?
    } else {
        let sha = sum(Path::new(&a.command[0]))?;
        (a.command, json!({"callerSha256":sha}))
    };
    let b = provenance["callerBinding"].as_str();
    let obs = observe(&cmd, cases, b)?;
    let failures = fails(cases, &obs)?;
    ensure!(failures.is_empty(), "failed public cases {failures:?}");
    let controls = controls(cases, &obs)?;
    let compressed = zlib(cases, &obs, &cmd, b)?;
    let peer = if a.peer_command.is_empty() {
        &cmd
    } else {
        &a.peer_command
    };
    let mut exchanges = json!({"writerToReader":mixed(cases,&cmd,peer,b,None)?});
    if !a.peer_command.is_empty() {
        exchanges["readerToWriter"] = mixed(cases, peer, &cmd, None, b)?;
    }
    let mut holdout = V::Null;
    if let (Some(rows), Some(path)) = (&sealed, &a.holdout) {
        let prefix = native::holdout_boundary(path)?;
        for (args, binding) in [(&cmd, b), (peer, None)]
            .into_iter()
            .take(if a.peer_command.is_empty() { 1 } else { 2 })
        {
            let mut combined = prefix.clone();
            combined.extend(args.clone());
            let result = observe(&combined, rows, binding).and_then(|o| fails(rows, &o));
            ensure!(result.is_ok_and(|f| f.is_empty()), "sealed replay failed");
        }
        holdout = json!({"cases":rows.len(),"fileSha256":sum(path)?,"passed":true,"directReadDenied":true,"peerCompared":!a.peer_command.is_empty()});
    }
    ensure!(
        provenance["callerSha256"] == sum(Path::new(&cmd[0]))?,
        "adapter changed during replay"
    );
    let mut sources = BTreeMap::new();
    for p in [
        "verification/marmish-inventory/src/bin/portable-oracle/main.rs",
        "verification/marmish-inventory/src/bin/portable-oracle/corpus.rs",
        "verification/marmish-inventory/src/bin/portable-oracle/native.rs",
        "verification/marmish-inventory/src/stream_observer.rs",
        "examples/marmish/portable-oracle/authored-cases.json",
    ] {
        sources.insert(p, sum(&root().join(p))?);
    }
    let mut report = json!({
        "schema":1,
        "provenance":provenance,
        "corpusRevision":public.freeze["revision"],
        "freezeSha256":sum(&directory().join("freeze.json"))?,
        "oracleSourceSha256":sources,
        "runtime":{"platform":std::env::consts::OS,"arch":std::env::consts::ARCH,"locale":"C","timezone":"UTC","environment":{"PATH":"/usr/bin:/bin"}},
        "priorCases":public.prior,
        "authoredCases":public.authored,
        "seededCases":public.seeded,
        "publicObservationSha256":hash(&obs)?,
        "negativeControls":controls,
        "alternateZlib":compressed,
        "mixedBinaryAndJson":exchanges,
        "holdout":holdout,
    });
    if let Some(path) = a.observations {
        let mut gz = GzEncoder::new(fs::File::create(&path)?, Compression::default());
        serde_json::to_writer(&mut gz, &obs)?;
        gz.write_all(b"\n")?;
        gz.finish()?;
        report["publicObservationArchiveSha256"] = json!(sum(&path)?);
    }
    fs::write(a.receipt, serde_json::to_string_pretty(&report)? + "\n")?;
    println!(
        "{} public stream cases; {} sealed cases; 3 detected mutations; {} wire replays",
        cases.len(),
        sealed.as_ref().map_or(0, Vec::len),
        array(&exchanges["writerToReader"])?.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    #[test]
    fn independent_expectations_replay_frozen_observations() -> Result<()> {
        let public = corpus::public()?;
        assert_eq!(
            (public.prior, public.authored, public.seeded),
            (50, 96, 257)
        );
        let old = marmish_inventory::read_json(&directory().join("original-receipt.json"))?;
        let mut bytes = vec![];
        flate2::read::GzDecoder::new(fs::File::open(
            directory().join("original-observations.json.gz"),
        )?)
        .read_to_end(&mut bytes)?;
        let observations: Vec<V> = serde_json::from_slice(&bytes)?;
        assert_eq!(hash(&observations)?, old["publicObservationSha256"]);
        assert!(fails(&public.cases, &observations)?.is_empty());
        assert_eq!(
            controls(&public.cases, &observations)?,
            old["negativeControls"]
        );
        Ok(())
    }
}
