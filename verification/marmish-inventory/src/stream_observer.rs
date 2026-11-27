//! Compare stream driver output against independently specified wire expectations.
use crate::{array, hex_encode, set, text};
use anyhow::{Context, Result, ensure};
use base64::{
    Engine, alphabet,
    engine::{GeneralPurpose, GeneralPurposeConfig},
};
use flate2::{Decompress, FlushDecompress, Status};
use serde_json::{Value, json};

pub fn expand(value: &Value) -> Result<Value> {
    match value {
        Value::Array(items) => Ok(Value::Array(
            items.iter().map(expand).collect::<Result<_>>()?,
        )),
        Value::Object(fields)
            if fields
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                == set(&["prefix", "byte", "repeat"]) =>
        {
            let byte = value["byte"].as_u64().context("invalid compact byte")?;
            ensure!(byte <= 255, "invalid compact byte");
            let count = usize::try_from(value["repeat"].as_u64().context("invalid byte repeat")?)?;
            Ok(json!(format!(
                "{}{}",
                text(&value["prefix"])?,
                format!("{byte:02x}").repeat(count)
            )))
        }
        Value::Object(fields) => Ok(Value::Object(
            fields
                .iter()
                .map(|(k, v)| Ok((k.clone(), expand(v)?)))
                .collect::<Result<_>>()?,
        )),
        other => Ok(other.clone()),
    }
}
fn zlib_matches(actual: &Value, expected: &Value) -> Result<bool> {
    let decoder = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    );
    let encoded = decoder.decode(text(&actual["data"])?)?;
    let mut raw = [0; 8193];
    let mut zlib = Decompress::new(true);
    let status = zlib.decompress(&encoded, &mut raw, FlushDecompress::Finish)?;
    let len = usize::try_from(zlib.total_out())?;
    Ok(status == Status::StreamEnd
        && zlib.total_in() == encoded.len() as u64
        && actual["codec"] == "zlib"
        && actual["offset"] == expected["offset"]
        && actual["raw_len"] == len
        && actual["sha256"] == expected["sha256"]
        && encoded.len() < len
        && expected["zlib_roundtrip_hex"] == hex_encode(&raw[..len]))
}
pub fn matches(actual: &Value, expected: &Value) -> bool {
    if expected.get("zlib_roundtrip_hex").is_some() {
        return zlib_matches(actual, expected).unwrap_or(false);
    }
    match (expected, actual) {
        (Value::Object(want), Value::Object(got)) => {
            want.len() == got.len()
                && want
                    .iter()
                    .all(|(k, v)| got.get(k).is_some_and(|x| matches(x, v)))
        }
        (Value::Array(want), Value::Array(got)) => {
            want.len() == got.len() && want.iter().zip(got).all(|(v, x)| matches(x, v))
        }
        _ => actual == expected,
    }
}
/// Replays the local writer's frozen bytes through the other role's decoder.
pub fn peer_cases(
    peer: &Value,
    cases: &[Value],
    vectors: &str,
    driver: &str,
    identity: &Value,
    binding: impl Fn(&Value) -> String,
) -> Result<Vec<Value>> {
    let other = match identity["role"].as_str() {
        Some("original") => "candidate",
        Some("candidate") => "original",
        _ => anyhow::bail!("wire replay requires distinct original and candidate roles"),
    };
    ensure!(
        peer["role"] == other,
        "wire replay requires distinct original and candidate roles"
    );
    ensure!(
        peer["vectors_sha256"] == vectors
            && peer["driver_source_sha256"] == driver
            && peer["all_matched"] == true,
        "wire replay requires a passing peer using the same frozen driver and vectors"
    );
    let provenance = &peer["workspace_identity"];
    ensure!(
        provenance["role"] == other
            && provenance["root"] != identity["root"]
            && ["cargoInputs", "sources", "baselineSha256"]
                .iter()
                .all(|k| provenance[k] == identity[k])
            && peer["driver_binding_sha256"]
                == binding(&json!({"workspace":provenance,"libraries":peer["libraries"]})),
        "peer receipt is not bound to a distinct build of the same frozen inputs"
    );
    let observed = array(&peer["observations"])?;
    ensure!(
        observed.len() == cases.len(),
        "wrong peer observation membership"
    );
    let mut index = std::collections::BTreeMap::new();
    for (row, case) in observed.iter().zip(cases) {
        ensure!(
            row["name"] == case["name"],
            "wrong peer observation membership"
        );
        ensure!(
            matches(&row["actual"], &expand(&case["expected"])?),
            "peer receipt observations do not match the frozen vectors"
        );
        index.insert(text(&case["name"])?, (case, &row["actual"]));
    }
    let lookup = |k: &str| {
        index
            .get(k)
            .copied()
            .with_context(|| format!("missing peer case {k}"))
    };
    let mut replay = vec![];
    for prefix in [
        "binary-data",
        "maximum-data",
        "checkpoint",
        "progress",
        "error-record",
        "hello-request",
        "gap-response",
    ] {
        let encoder = format!("{prefix}-encode");
        let (_, actual) = lookup(&encoder)?;
        let (case, _) = lookup(&format!("{prefix}-decode"))?;
        let mut case = case.clone();
        case["name"] = json!(format!("{other}:{encoder}"));
        case["input"] = actual.clone();
        replay.push(case);
    }
    for encoder in ["codec-uncompressed-encode", "codec-zlib-encode"] {
        let (case, actual) = lookup(encoder)?;
        replay.push(json!({"name":format!("{other}:{encoder}"),"operation":"codec_decode","input":{"block":actual,"negotiated":"zlib"},"expected":case["input"]["bytes"]}));
    }
    Ok(replay)
}
