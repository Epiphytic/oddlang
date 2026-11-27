use crate::{
    corpus::{case, hash, invalid},
    native::{command, observe, run},
    sandbox::environment,
};
use anyhow::{Context, Result, bail, ensure};
use marmish_inventory::{
    array, hex_decode,
    stream_observer::{expand, matches},
    text,
};
use serde_json::{Value as V, json};
use std::{collections::BTreeMap, path::Path};
pub type Adapters = BTreeMap<String, Vec<String>>;
pub fn comparable(row: &V) -> Result<bool> {
    Ok(match text(&row["operation"])? {
        "frame_encode" => row["input"]["kind"] == "data",
        "frame_decode" => {
            let raw = hex_decode(text(&expand(&row["input"])?)?)?;
            raw.len() < 5 || raw[0] == 5 || !(1..=11).contains(&raw[0])
        }
        "frame_fragments" => array(&row["expected"])?
            .iter()
            .all(|s| s["frame"].is_null() || s["frame"]["kind"] == "data"),
        _ => false,
    })
}
fn lines(row: &V) -> Result<Vec<String>> {
    let v = expand(&row["input"])?;
    Ok(match text(&row["operation"])? {
        "frame_encode" => vec![format!("E {} {}", v["offset"], text(&v["bytes"])?)],
        "frame_decode" => vec![format!("D {}", text(&v)?)],
        "frame_fragments" => {
            let mut lines = vec!["B".into()];
            for s in array(&v)? {
                lines.push(if s == "finish" {
                    "X".into()
                } else {
                    format!("F {}", text(s)?)
                })
            }
            lines
        }
        _ => bail!("unknown operation"),
    })
}
fn record(v: &V) -> Result<String> {
    ensure!(v["kind"] == "data", "non-Data frame");
    Ok(format!("5:{}:{}", v["offset"], text(&v["bytes"])?))
}
fn result(v: &V, success: impl FnOnce() -> Result<String>) -> Result<String> {
    if let Some(error) = v.get("error") {
        let detail = v["detail"]
            .as_str()
            .map(|s| format!("/{s}"))
            .unwrap_or_default();
        return Ok(format!("!{}{detail}", text(error)?));
    }
    success()
}
fn expected(row: &V) -> Result<Vec<String>> {
    let v = expand(&row["expected"])?;
    Ok(match text(&row["operation"])? {
        "frame_encode" => vec![result(&v, || Ok(format!("E:{}", text(&v)?)))?],
        "frame_decode" => vec![result(&v, || Ok(format!("D:{}", record(&v)?)))?],
        "frame_fragments" => {
            let mut lines = vec!["B".into()];
            for s in array(&v)? {
                lines.push(result(s, || {
                    if s == "ok" {
                        return Ok("X".into());
                    }
                    let frame = if s["frame"].is_null() {
                        String::new()
                    } else {
                        record(&s["frame"])?
                    };
                    Ok(format!("F:{}:{frame}", s["consumed"]))
                })?);
            }
            lines
        }
        _ => bail!("unknown operation"),
    })
}
pub fn rows(stage: &Path, args: &[String], requests: &[String]) -> Result<Vec<String>> {
    ensure!(
        requests
            .iter()
            .all(|s| s.is_ascii() && !s.contains(['\r', '\n'])),
        "expected single-line adapter requests"
    );
    let mut c = command(args)?;
    environment(&mut c, stage, None);
    let (s, o, e) = run(&mut c, (requests.join("\n") + "\n").as_bytes(), 30)?;
    ensure!(
        s.success() && e.is_empty() && o.is_ascii(),
        "target failed or wrote stderr"
    );
    let rows: Vec<_> = std::str::from_utf8(&o)?
        .lines()
        .map(str::to_owned)
        .collect();
    ensure!(rows.len() == requests.len(), "observation count differs");
    Ok(rows)
}
pub fn assert_rows(stage: &Path, args: &[String], cases: &[V]) -> Result<Vec<String>> {
    let mut input = vec![];
    let mut want = vec![];
    for c in cases {
        input.extend(lines(c)?);
        want.extend(expected(c)?);
    }
    let out = rows(stage, args, &input)?;
    ensure!(
        out == want,
        "target disagrees with expected Data observations"
    );
    Ok(out)
}
pub fn original(args: &[String], cases: &[V], binding: &str) -> Result<Vec<V>> {
    let out = observe(args, cases, Some(binding))?;
    for (c, o) in cases.iter().zip(&out) {
        ensure!(
            matches(&o["actual"], &expand(&c["expected"])?),
            "upstream disagrees with expectations"
        );
    }
    Ok(out)
}
pub fn peers(
    stage: &Path,
    adapters: &Adapters,
    public: &[V],
    upstream: &[String],
    binding: &str,
) -> Result<V> {
    let writes: Vec<_> = public
        .iter()
        .filter(|r| r["operation"] == "frame_encode" && r["expected"].get("error").is_none())
        .cloned()
        .collect();
    let mut outputs = BTreeMap::new();
    for (name, args) in adapters {
        let observed = assert_rows(stage, args, &writes)?;
        outputs.insert(
            name.as_str(),
            observed
                .iter()
                .map(|s| Ok(s.strip_prefix("E:").context("encode prefix")?.to_owned()))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    outputs.insert(
        "upstream",
        original(upstream, &writes, binding)?
            .iter()
            .map(|r| Ok(text(&r["actual"])?.to_owned()))
            .collect::<Result<_>>()?,
    );
    let mut report = json!({});
    for (writer, packets) in &outputs {
        let reads = writes
            .iter()
            .zip(packets)
            .map(|(c, p)| {
                Ok(case(
                    format!("peer:{}", text(&c["name"])?),
                    "frame_decode",
                    json!(p),
                    expand(&c["input"])?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for reader in ["rust", "typescript", "upstream"] {
            if reader == *writer {
                continue;
            }
            let decoded = if reader == "upstream" {
                original(upstream, &reads, binding)?
                    .iter()
                    .map(|r| Ok(format!("D:{}", record(&r["actual"])?)))
                    .collect::<Result<Vec<_>>>()?
            } else {
                assert_rows(stage, &adapters[reader], &reads)?
            };
            report[format!("{writer}To{}{}", reader[..1].to_uppercase(), &reader[1..])] = json!({"records":packets.len(),"wireSha256":hash(packets)?,"decodedRowsSha256":hash(&decoded)?});
        }
    }
    Ok(report)
}
pub fn mutation(
    release: &Path,
    alternate: &Path,
    adapters: &Adapters,
    mutants: &Adapters,
    w: &V,
) -> Result<Vec<V>> {
    let original = text(&w["expected"])?;
    let changed = format!("33{}", &original[2..]);
    let offset = &w["input"]["offset"];
    let payload = text(&w["input"]["bytes"])?;
    let decoded = format!("51:{offset}:{payload}");
    let mut cases = vec![];
    for (name, args) in mutants {
        let input = vec![
            lines(w)?[0].clone(),
            format!("D {original}"),
            format!("D {changed}"),
            "B".into(),
            format!("F {}", &changed[..8]),
            format!("F {}", &changed[8..]),
            "X".into(),
            "B".into(),
            format!("F {original}"),
            format!("F {changed}"),
            "X".into(),
            format!("T 5 {offset} {payload}"),
        ];
        let unknown = "!Invalid/copy_frame_tag".to_string();
        let want = vec![
            format!("E:{changed}"),
            unknown.clone(),
            format!("D:{decoded}"),
            "B".into(),
            "F:4:".into(),
            format!("F:{}:{decoded}", changed.len() / 2 - 4),
            "X".into(),
            "B".into(),
            unknown,
            "!Invalid/copy_decoder_failed".into(),
            "!Invalid/copy_truncated".into(),
            "!Invalid/value".into(),
        ];
        ensure!(
            rows(alternate, args, &input)? == want
                && rows(
                    release,
                    &adapters[name],
                    &[format!("T 51 {offset} {payload}")]
                )? == ["!Invalid/value"],
            "tag mutation behavior differs for {name}"
        );
        cases.push(case(
            format!("{name}-changed-tag"),
            "frame_decode",
            json!(changed),
            invalid("copy_frame_tag"),
        ));
    }
    Ok(cases)
}
