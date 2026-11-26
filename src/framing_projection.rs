use crate::framing_import::{Framing, load_verified};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fmt::Write, fs, path::Path};

const WIT: &str = "package oddlang:framing@1.0.0;

interface codec {
    enum frame-error { size-limit, invalid-json, invalid-version, invalid-encoding }
    encode: func(input: list<u8>) -> result<list<u8>, frame-error>;
    decode: func(input: list<u8>) -> result<list<u8>, frame-error>;
}

world framing {
    export codec;
}
";

fn module(model: &Framing) -> Result<String, String> {
    // Every supported target includes wasm32. The encoded-length check must also fit usize.
    if model.raw_limit.div_ceil(3) > u64::from(u32::MAX) / 4
        || model.json_limit > u64::from(u32::MAX)
        || model.ndjson_limit > u64::from(u32::MAX)
    {
        return Err("framing: declared limits exceed the target integer range".into());
    }
    let mut out = String::from(
        "// Generated from the locked Oddlang framing contract.\n\
         use base64::{engine::general_purpose::STANDARD, Engine};\n\
         use serde::{Deserialize, Serialize};\n\n\
         #[derive(Clone, Copy, Debug, Default)]\npub struct Limits;\n\nimpl Limits {\n",
    );
    for (name, limit) in [
        ("RAW_BYTES", model.raw_limit),
        ("JSON_BYTES", model.json_limit),
        ("NDJSON_BYTES", model.ndjson_limit),
    ] {
        writeln!(out, "    pub const {name}: usize = {limit};").unwrap();
    }
    out.push_str("}\n\n#[derive(Debug, PartialEq, Eq, thiserror::Error)]\npub enum FrameError {\n");
    for (name, message) in [
        "SizeLimit",
        "InvalidJson",
        "InvalidVersion",
        "InvalidEncoding",
    ]
    .iter()
    .zip(&model.errors)
    {
        writeln!(out, "    #[error({message:?})]\n    {name},").unwrap();
    }
    out.push_str(
        "}\n\n#[derive(Serialize, Deserialize)]\n#[serde(deny_unknown_fields)]\n\
         struct WireFrame {\n    version: u32,\n    payload: String,\n}\n\n\
         pub struct ProbeEnvelope;\n\nimpl ProbeEnvelope {\n\
         \x20   pub fn encode(input: &[u8]) -> Result<Vec<u8>, FrameError> {\n",
    );
    for step in &model.encode_steps {
        let line = match step.as_str() {
            "raw-limit" => {
                "if input.len() > Limits::RAW_BYTES { return Err(FrameError::SizeLimit); }".into()
            }
            "json-encode-base64" => format!(
                "let record = WireFrame {{ version: {}, payload: STANDARD.encode(input) }};\n        let frame = serde_json::to_vec(&record).map_err(|_| FrameError::InvalidJson)?;",
                model.version
            ),
            "json-limit" => {
                "if frame.len() > Limits::JSON_BYTES { return Err(FrameError::SizeLimit); }".into()
            }
            "return-frame" => "Ok(frame)".into(),
            other => return Err(format!("framing: unknown encode operation {other}")),
        };
        writeln!(out, "        {line}").unwrap();
    }
    out.push_str("    }\n\n    pub fn decode(input: &[u8]) -> Result<Vec<u8>, FrameError> {\n");
    for step in &model.decode_steps {
        let line = match step.as_str() {
            "json-limit" => "if input.len() > Limits::JSON_BYTES { return Err(FrameError::SizeLimit); }".into(),
            "json-decode-closed" => "let record: WireFrame = serde_json::from_slice(input).map_err(|_| FrameError::InvalidJson)?;".into(),
            "version" => format!("if record.version != {} {{ return Err(FrameError::InvalidVersion); }}", model.version),
            "encoded-limit" => "if record.payload.len() > Limits::RAW_BYTES.div_ceil(3) * 4 { return Err(FrameError::SizeLimit); }".into(),
            "base64-decode" => "let payload = STANDARD.decode(&record.payload).map_err(|_| FrameError::InvalidEncoding)?;".into(),
            "raw-limit" => "if payload.len() > Limits::RAW_BYTES { return Err(FrameError::SizeLimit); }".into(),
            "canonical" => "if STANDARD.encode(&payload) != record.payload { return Err(FrameError::InvalidEncoding); }".into(),
            "return-bytes" => "Ok(payload)".into(),
            other => return Err(format!("framing: unknown decode operation {other}")),
        };
        writeln!(out, "        {line}").unwrap();
    }
    out.push_str("    }\n}\n");
    Ok(out)
}

pub fn build(import: &Path, output: &Path) -> Result<(), String> {
    write_projection(load_verified(import)?, output)
}

pub fn build_pinned(import: &Path, expected_lock: &str, output: &Path) -> Result<(), String> {
    write_projection(
        crate::framing_import::load_pinned(import, expected_lock)?,
        output,
    )
}

fn write_projection((model, graph_hash): (Framing, String), output: &Path) -> Result<(), String> {
    let rust = module(&model)?;
    if let Ok(meta) = fs::symlink_metadata(output) {
        if !meta.is_dir()
            || fs::read_dir(output)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err("framing: projection requires an empty output directory".into());
        }
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let rust_hash: String = Sha256::digest(rust.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let manifest = json!({"version":1,"graphSha256":graph_hash,
                          "files":{"frame.rs":rust_hash}});
    for (path, bytes) in [
        ("frame.rs", rust),
        ("world.wit", WIT.into()),
        (
            "projection.json",
            serde_json::to_string_pretty(&manifest).unwrap() + "\n",
        ),
    ] {
        fs::write(stage.path().join(path), bytes).map_err(|e| e.to_string())?;
    }
    fs::rename(stage.path(), output).map_err(|e| e.to_string())
}

pub fn verify_build(import: &Path, output: &Path) -> Result<(), String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let expected = scratch.path().join("expected");
    build(import, &expected)?;
    compare_directories(&expected, output)
}

pub fn compare_directories(expected: &Path, actual: &Path) -> Result<(), String> {
    use std::collections::BTreeMap;
    fn read(root: &Path) -> Result<BTreeMap<std::ffi::OsString, Vec<u8>>, String> {
        if !fs::symlink_metadata(root)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return Err("framing: expected a directory".into());
        }
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                return Err("framing: unexpected artifact type".into());
            }
            files.insert(
                entry.file_name(),
                fs::read(entry.path()).map_err(|e| e.to_string())?,
            );
        }
        Ok(files)
    }
    if read(expected)? != read(actual)? {
        return Err("framing: artifacts differ from verified production inputs".into());
    }
    Ok(())
}
