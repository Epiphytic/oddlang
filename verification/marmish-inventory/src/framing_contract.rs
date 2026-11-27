//! Frozen byte-segment expressions and independent framing expectations.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().context("missing text")
}
fn list(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("missing array")
}
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").unwrap();
    }
    out
}
pub fn expand(segments: &Value) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for segment in list(segments)? {
        let obj = segment.as_object().context("invalid byte segment")?;
        if obj.len() == 1 && obj.contains_key("utf8") {
            out.extend(text(&segment["utf8"])?.as_bytes());
            continue;
        }
        let (s, count) = if obj.len() == 1 && obj.contains_key("hex") {
            (text(&segment["hex"])?, 1)
        } else if obj.len() == 2 && obj.contains_key("repeatHex") {
            (
                text(&segment["repeatHex"])?,
                segment["count"].as_u64().context("invalid repeat count")?,
            )
        } else {
            bail!("invalid byte segment")
        };
        let digits: Vec<_> = s
            .split_ascii_whitespace()
            .flat_map(str::as_bytes)
            .copied()
            .collect();
        ensure!(digits.len() % 2 == 0, "odd hex length");
        let bytes = digits
            .chunks_exact(2)
            .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
            .collect::<Result<Vec<_>>>()?;
        for _ in 0..count {
            out.extend_from_slice(&bytes);
        }
    }
    Ok(out)
}
pub fn cases(contract: &Value) -> Result<Value> {
    let mut rows = Vec::new();
    for case in list(&contract["cases"])? {
        let v = &case["expected"];
        let actual = if let Some(bytes) = v.get("bytes") {
            json!({"bytesHex":hex(&expand(bytes)?)})
        } else {
            v.clone()
        };
        rows.push(json!({"id":case["id"],"operation":case["operation"],"observed":actual,"inputSha256":hash(&expand(&case["input"])?) }));
    }
    Ok(json!(rows))
}
