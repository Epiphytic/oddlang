//! Independent expectations. Only stimuli and independently authored outcomes enter this module.
use anyhow::{Context, Result, ensure};
use marmish_inventory::{PIN, array, digest, hex_decode, hex_encode, read, read_json, root, text};
use serde_json::{Value as V, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub fn directory() -> PathBuf {
    root().join("examples/marmish/portable-oracle")
}
pub fn hash(value: &impl serde::Serialize) -> Result<String> {
    use std::fmt::Write;
    let mut document = serde_json::to_value(value)?;
    document.sort_all_objects();
    let mut canonical = String::new();
    for ch in serde_json::to_string(&document)?.chars() {
        if ch < '\u{7f}' {
            canonical.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                write!(canonical, "\\u{unit:04x}")?;
            }
        }
    }
    Ok(digest(canonical.as_bytes()))
}
pub fn case(name: impl AsRef<str>, operation: &str, input: V, expected: V) -> V {
    json!({"name":name.as_ref(),"operation":operation,"input":input,"expected":expected})
}
fn err(name: &str) -> V {
    json!({"error":name})
}
pub fn invalid(detail: &str) -> V {
    json!({"error":"Invalid","detail":detail})
}
fn authored() -> Result<Vec<V>> {
    let fixture = read_json(&directory().join("authored-cases.json"))?;
    let mut cases = vec![];
    for mut c in array(&fixture["cases"])?.clone() {
        let pair = c.as_object_mut().context("case")?.remove("pairedDecode");
        cases.push(c.clone());
        if pair == Some(json!(true)) {
            let name = text(&c["name"])?
                .strip_suffix("-encode")
                .context("paired encoder name")?;
            let op = text(&c["operation"])?
                .strip_suffix("_encode")
                .context("paired encoder op")?;
            cases.push(case(
                format!("{name}-decode"),
                &format!("{op}_decode"),
                c["expected"].clone(),
                c["input"].clone(),
            ));
        }
    }
    let mut enums = fixture["numberedEnums"].clone();
    for names in enums["expected"]
        .as_object_mut()
        .context("enum table")?
        .values_mut()
    {
        let mut domain = vec![invalid("copy_enum"); 256];
        let valid = array(names)?.clone();
        ensure!(valid.len() < 256, "enum overflow");
        domain[1..1 + valid.len()].clone_from_slice(&valid);
        *names = json!(domain);
    }
    cases.push(enums);
    Ok(cases)
}
struct Seed {
    bytes: Vec<u8>,
    counter: u64,
}
impl Seed {
    fn new(s: &str) -> Result<Self> {
        let bytes = hex_decode(s)?;
        ensure!(bytes.len() == 32, "expected 32-byte seed");
        Ok(Self { bytes, counter: 0 })
    }
    fn take(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            out.extend(
                Sha256::new()
                    .chain_update(&self.bytes)
                    .chain_update(self.counter.to_be_bytes())
                    .finalize(),
            );
            self.counter += 1;
        }
        out.truncate(n);
        out
    }
    fn below(&mut self, n: u64) -> u64 {
        u64::from_be_bytes(self.take(8).try_into().expect("eight seed bytes")) % n
    }
}
fn packet(offset: u64, bytes: &[u8]) -> Vec<u8> {
    let mut p = vec![5];
    p.extend((8 + bytes.len() as u32).to_be_bytes());
    p.extend(offset.to_be_bytes());
    p.extend(bytes);
    p
}
fn state(accepted: usize, delivered: usize, eof: Option<usize>) -> V {
    json!({"accepted":accepted,"delivered":delivered,"eof":eof,"drained":eof==Some(delivered),"buffered":accepted-delivered,"credit":if eof.is_some(){0}else{262144-accepted+delivered}})
}
fn round(s: &mut Seed, index: u64, cases: &mut Vec<V>) {
    let len = 1 + s.below(129);
    let raw = s.take(len as usize);
    let interior = s.below(1 << 32);
    let offset = [0, i64::MAX as u64 - len, interior][index as usize % 3];
    let other = s.take(5);
    let first = json!({"kind":"data","offset":offset,"bytes":hex_encode(&raw)});
    let second = json!({"kind":"data","offset":71,"bytes":hex_encode(&other)});
    let p = packet(offset, &raw);
    let q = packet(71, &other);
    let cut = 1 + s.below(p.len() as u64 - 1) as usize;
    let mut bad = p.clone();
    bad[0] = 12 + s.below(244) as u8;
    let mut add = |name: &str, op: &str, input: V, expected: V| {
        cases.push(case(format!("seed-{index}-{name}"), op, input, expected))
    };
    add(
        "data-write",
        "frame_encode",
        first.clone(),
        json!(hex_encode(&p)),
    );
    add(
        "data-read",
        "frame_decode",
        json!(hex_encode(&p)),
        first.clone(),
    );
    let pending = json!({"consumed":cut,"frame":null});
    let complete = json!({"consumed":p.len()-cut,"frame":first});
    add(
        "fragmented",
        "frame_fragments",
        json!([
            hex_encode(&p[..cut]),
            hex_encode(&[&p[cut..], &q].concat()),
            hex_encode(&q),
            "finish"
        ]),
        json!([pending,complete,{"consumed":q.len(),"frame":second},"ok"]),
    );
    add(
        "uncompleted-finish-retry",
        "frame_fragments",
        json!([
            hex_encode(&p[..cut]),
            "finish",
            hex_encode(&p[cut..]),
            "finish"
        ]),
        json!([pending, invalid("copy_truncated"), complete, "ok"]),
    );
    add(
        "unknown-tag",
        "frame_fragments",
        json!([hex_encode(&bad), hex_encode(&p), "finish"]),
        json!([
            invalid("copy_frame_tag"),
            invalid("copy_decoder_failed"),
            invalid("copy_truncated")
        ]),
    );
    add(
        "truncated-tag-precedence",
        "frame_decode",
        json!(hex_encode(&bad[..bad.len() - 1])),
        invalid("copy_frame_length"),
    );
    add(
        "offset-overflow",
        "frame_decode",
        json!(hex_encode(&packet(i64::MAX as u64 - len + 1, &raw))),
        err("Offset"),
    );
    let end = 2 + s.below(80) as usize;
    let raw = s.take(end);
    let split = 1 + s.below(end as u64 - 1) as usize;
    let mut altered = raw.clone();
    altered[0] ^= 1;
    let ops = json!([
        {"op":"accept","offset":0,"bytes":hex_encode(&raw)},{"op":"deliver","offset":split},
        {"op":"accept","offset":0,"bytes":hex_encode(&altered)},{"op":"state"},
        {"op":"finish","offset":end+1},{"op":"accept","offset":end+1,"bytes":"00"},
        {"op":"deliver","offset":end+1},{"op":"state"},{"op":"accept","offset":0,"bytes":hex_encode(&raw)},
        {"op":"finish","offset":end},{"op":"accept","offset":end,"bytes":"00"},{"op":"accept","offset":0,"bytes":hex_encode(&raw)},
        {"op":"pending","limit":end},{"op":"deliver","offset":end},{"op":"state"}
    ]);
    add(
        "receive-retry",
        "receive_window",
        ops,
        json!([
            true,
            "ok",
            err("Conflict"),
            state(end, split, None),
            err("Eof"),
            err("Gap"),
            err("Ack"),
            state(end, split, None),
            false,
            "ok",
            err("Eof"),
            false,
            hex_encode(&raw[split..]),
            "ok",
            state(end, end, Some(end))
        ]),
    );
}
fn capacity() -> V {
    let mut ops: Vec<_> = (0..256)
        .map(|n| json!({"op":"accept","offset":n,"bytes":"66"}))
        .collect();
    ops.extend([
        json!({"op":"state"}),
        json!({"op":"accept","offset":256,"bytes":"66"}),
        json!({"op":"deliver","offset":1}),
        json!({"op":"accept","offset":256,"bytes":"77"}),
        json!({"op":"accept","offset":0,"bytes":"66"}),
        json!({"op":"accept","offset":1,"bytes":"66"}),
        json!({"op":"state"}),
    ]);
    let mut answers = vec![json!(true); 256];
    let full = |n: usize, d: usize| json!({"accepted":n,"delivered":d,"buffered":256,"credit":0,"eof":null,"drained":false});
    answers.extend([
        full(256, 0),
        err("Limit"),
        json!("ok"),
        json!(true),
        err("OldRetry"),
        json!(false),
        full(257, 1),
    ]);
    case(
        "receipt-capacity-eviction",
        "receive_window",
        json!(ops),
        json!(answers),
    )
}
fn seeded(seed: &str, rounds: u64) -> Result<Vec<V>> {
    let mut s = Seed::new(seed)?;
    let mut cases = vec![];
    for i in 0..rounds {
        round(&mut s, i, &mut cases);
    }
    cases.push(capacity());
    Ok(cases)
}
pub struct Public {
    pub cases: Vec<V>,
    pub prior: usize,
    pub authored: usize,
    pub seeded: usize,
    pub freeze: V,
}
pub fn public() -> Result<Public> {
    let dir = directory();
    let freeze = read_json(&dir.join("freeze.json"))?;
    let path = dir.join("../stream-vectors.json");
    let prior = read_json(&path)?;
    ensure!(
        freeze["schema"] == 1
            && freeze["revision"] == PIN
            && prior["schema"] == 1
            && prior["upstream_commit"] == PIN
            && digest(&read(&path)?) == freeze["priorCorpusSha256"],
        "prior corpus pin mismatch"
    );
    let spec = authored()?;
    let seed = seeded(
        text(&freeze["seed"])?,
        freeze["seededRounds"].as_u64().context("rounds")?,
    )?;
    ensure!(
        hash(&spec)? == freeze["authoredCorpusSha256"]
            && hash(&seed)? == freeze["seededCorpusSha256"],
        "independent corpus commitment mismatch"
    );
    let mut cases = array(&prior["cases"])?.clone();
    let counts = (cases.len(), spec.len(), seed.len());
    cases.extend(spec);
    cases.extend(seed);
    let ids: BTreeSet<_> = cases.iter().map(|r| r["name"].as_str()).collect();
    ensure!(
        ids.len() == cases.len() && !ids.contains(&None),
        "duplicate case identifier"
    );
    Ok(Public {
        cases,
        prior: counts.0,
        authored: counts.1,
        seeded: counts.2,
        freeze,
    })
}
pub fn sealed(path: &Path, freeze: &V) -> Result<Vec<V>> {
    let data = read(path)?;
    let f = &freeze["holdout"];
    ensure!(
        digest(&data) == f["fileSha256"],
        "sealed file commitment mismatch"
    );
    let capsule: V = serde_json::from_slice(&data)?;
    let mut rows = seeded(
        text(&capsule["seed"])?,
        capsule["rounds"].as_u64().context("sealed rounds")?,
    )?;
    rows.pop();
    for r in &mut rows {
        r["name"] = json!(text(&r["name"])?.replacen("seed-", "holdout-", 1));
    }
    ensure!(
        f["cases"] == rows.len()
            && hash(&rows)? == f["corpusSha256"]
            && capsule["cases"] == json!(rows),
        "sealed derivation mismatch"
    );
    Ok(rows)
}
