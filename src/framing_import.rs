//! Import and verify the closed framing-v1 Rust profile.
mod cargo;
mod graph;
mod syntax;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Framing {
    pub raw_limit: u64,
    pub json_limit: u64,
    pub ndjson_limit: u64,
    pub version: u32,
    pub errors: [String; 4],
    pub encode_steps: Vec<String>,
    pub decode_steps: Vec<String>,
}
fn valid_message(msg: &str) -> bool {
    !msg.is_empty()
        && msg.len() <= 96
        && msg
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b" -.,:".contains(&c))
}
impl Framing {
    fn validate(&self) -> Result<()> {
        let encode = [
            "raw-limit",
            "json-encode-base64",
            "json-limit",
            "return-frame",
        ];
        let decode = [
            "json-limit",
            "json-decode-closed",
            "version",
            "encoded-limit",
            "base64-decode",
            "raw-limit",
            "canonical",
            "return-bytes",
        ];
        let mut reordered = decode;
        reordered.swap(2, 3);
        if self.encode_steps != encode
            || (self.decode_steps != decode && self.decode_steps != reordered)
            || self.errors.iter().any(|s| !valid_message(s))
            || self.raw_limit > u64::from(u32::MAX) / 4 * 3
            || self.json_limit > u32::MAX.into()
            || self.ndjson_limit > u32::MAX.into()
        {
            return Err("invalid framing-v1 limits, operation order or display values".into());
        }
        Ok(())
    }
}
fn hash(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
fn read(p: &Path) -> Result<Vec<u8>> {
    let md = fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !md.is_file() {
        return Err(format!("expected regular file: {}", p.display()));
    }
    fs::read(p).map_err(|e| e.to_string())
}
fn json(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    profile: String,
    package: String,
    source: String,
    module: String,
    codec: String,
    error: String,
    json_record: String,
    limits: String,
    base64: String,
    serde: String,
}
impl Mapping {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let m: Self = toml::from_str(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
            .map_err(|e| format!("framing mapping: {e}"))?;
        if m.profile != "framing-v1"
            || m.source != "src/frame.rs"
            || m.module != "frame"
            || m.codec != "ProbeEnvelope"
            || m.error != "FrameError"
            || m.json_record != "Envelope"
            || m.limits != "Limits"
            || m.base64 != "base64::engine::general_purpose::STANDARD"
            || m.serde != "closed-record"
        {
            return Err("unsupported framing-v1 mapping".into());
        }
        Ok(m)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lock {
    version: u8,
    profile: String,
    graph_sha256: String,
    files: BTreeMap<String, String>,
    mapping_sha256: String,
    recognizers: BTreeMap<String, String>,
    build: cargo::Build,
}
pub fn import(manifest: &Path, mapping: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        return Err("framing import requires a fresh output directory".into());
    }
    let bytes = read(mapping)?;
    let map = Mapping::parse(&bytes)?;
    let manifest = manifest.canonicalize().map_err(|e| e.to_string())?;
    let root = manifest.parent().ok_or("missing package directory")?;
    let frame = read(&root.join(&map.source))?;
    let (f, coords) =
        syntax::lower(std::str::from_utf8(&frame).map_err(|_| "source must be UTF-8")?)?;
    let build = cargo::check(&manifest, &map.package, &frame)?;
    let graph = f.graph();
    let metadata = json(&coords)?;
    let files = BTreeMap::from([
        ("domain.ttl".to_owned(), hash(graph.as_bytes())),
        ("source-map.json".to_owned(), hash(&metadata)),
    ]);
    let lock = Lock {
        version: 1,
        profile: "framing-v1".into(),
        graph_sha256: files["domain.ttl"].clone(),
        files,
        mapping_sha256: hash(&bytes),
        recognizers: primitive_rules(),
        build,
    };
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, data) in [
        ("domain.ttl", graph.into_bytes()),
        ("source-map.json", metadata),
        ("import.lock", json(&lock)?),
    ] {
        fs::write(temp.path().join(name), data).map_err(|e| e.to_string())?;
    }
    verify(temp.path())?;
    fs::rename(temp.path(), out).map_err(|e| e.to_string())
}
pub fn load_verified(dir: &Path) -> Result<(Framing, String)> {
    let mut names = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|f| f.file_name()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    names.sort();
    if names != ["domain.ttl", "import.lock", "source-map.json"] {
        return Err("unexpected framing import inventory".into());
    }
    let lock: Lock = serde_json::from_slice(&read(&dir.join("import.lock"))?)
        .map_err(|e| format!("invalid framing lock: {e}"))?;
    let bytes = read(&dir.join("domain.ttl"))?;
    let digest = hash(&bytes);
    let coords = read(&dir.join("source-map.json"))?;
    let files = BTreeMap::from([
        ("domain.ttl".to_owned(), digest.clone()),
        ("source-map.json".to_owned(), hash(&coords)),
    ]);
    if lock.profile != "framing-v1"
        || lock.version != 1
        || files != lock.files
        || lock.graph_sha256 != digest
    {
        return Err("stale framing import lock".into());
    }
    lock.build.validate()?;
    if lock.recognizers != primitive_rules() || !valid_digest(&lock.mapping_sha256) {
        return Err("invalid framing recognizer lock".into());
    }
    let model = Framing::from_graph(&bytes)?;
    syntax::validate_map(&coords, &model)?;
    Ok((model, digest))
}
pub fn load(dir: &Path) -> Result<Framing> {
    load_verified(dir).map(|v| v.0)
}
pub fn verify(dir: &Path) -> Result<()> {
    load_verified(dir).map(|_| ())
}

fn valid_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn primitive_rules() -> BTreeMap<String, String> {
    [
        (
            "serde_derive@1.0.229::Serialize",
            "named struct; fields version:u32 then payload:String; declaration order",
        ),
        (
            "serde_derive@1.0.229::Deserialize",
            "deny_unknown_fields; require both fields; reject duplicate keys and out-of-range u32",
        ),
        (
            "serde_json@1.0.151::to_vec",
            "compact UTF-8 bytes in declaration order; map_err InvalidJson",
        ),
        (
            "serde_json@1.0.151::from_slice",
            "single complete JSON record with surrounding whitespace; map_err InvalidJson",
        ),
        (
            "thiserror-impl@2.0.21::Error",
            "four unit alternatives with literal error attributes; Display and Error",
        ),
        (
            "base64@0.22.1::Engine::encode",
            "engine::general_purpose::STANDARD; RFC4648 padded standard alphabet",
        ),
        (
            "base64@0.22.1::Engine::decode",
            "STANDARD; reject invalid alphabet, padding and trailing bits; map_err InvalidEncoding",
        ),
        (
            "rustc::usize::div_ceil",
            "checked encoded length ceiling from bounded RAW_BYTES",
        ),
        (
            "rustc::slice::len",
            "byte slice and Vec<u8> lengths in octets",
        ),
        (
            "rustc::String::len",
            "UTF-8 byte count; re-encoded canonical string compared by value",
        ),
        (
            "rustc::derive",
            "Limits Clone Copy Debug Default; FrameError Debug PartialEq Eq",
        ),
        (
            "rustc::Result::map_err",
            "wildcard closure constant named error; ? returns the first error",
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect()
}
