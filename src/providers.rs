//! Workspace-selected providers. Loading, invoking and publishing are separate operations.
mod schema;
#[path = "strict_json.rs"]
mod strict_json;

use crate::native_package::digest;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const MIB: usize = 1024 * 1024;
use crate::provider_runtime::{MAX_COMPONENT_BYTES, MAX_DOCUMENT_BYTES as DOCUMENT_LIMIT};
const FILE_LIMIT: usize = 256;
const RECEIPT: &str = "provider-receipt.json";

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let strict_json::Strict(value) = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn read_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("expected regular provider input file".into());
    }
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("provider file size limit".into());
    }
    Ok(bytes)
}

fn logical_path(name: &str) -> Result<()> {
    if name.len() > 512
        || name.split('/').any(|part| {
            part.is_empty()
                || part.len() > 128
                || part.starts_with('.')
                || part.ends_with('.')
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || matches!(
                    part.split('.')
                        .next()
                        .unwrap()
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
    {
        return Err(format!("invalid provider relative path {name:?}"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    path: String,
    sha256: String,
}

impl Pin {
    fn read(&self, base: &Path, limit: usize) -> Result<Vec<u8>> {
        logical_path(&self.path)?;
        let path = base
            .join(&self.path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(base) {
            return Err("provider pin escapes registry directory".into());
        }
        let bytes = read_file(&path, limit)?;
        if digest(&bytes) != self.sha256 {
            return Err(format!("provider digest mismatch for {}", self.path));
        }
        Ok(bytes)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    role: String,
    capability: String,
    version: String,
    world: String,
    component: Pin,
    schema: Pin,
    configuration: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    providers: Vec<Registration>,
}

struct External {
    registration: Registration,
    component: Vec<u8>,
    schema: schema::Schema,
}

#[derive(Default)]
pub struct Registry {
    entries: BTreeMap<(String, String, String), External>,
}

#[derive(Default, Debug)]
pub struct ProviderOutput {
    pub artifacts: BTreeMap<String, Vec<u8>>,
    pub exports: Vec<String>,
    pub dependencies: Vec<String>,
    pub evidence: Vec<String>,
    pub gaps: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    base64: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    format: String,
    artifacts: Vec<Artifact>,
    exports: Vec<String>,
    dependencies: Vec<String>,
    evidence: Vec<String>,
    gaps: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    code: String,
    message: String,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    format: String,
    diagnostics: Vec<Diagnostic>,
}

fn decode_response(bytes: &[u8]) -> Result<Response> {
    let value: Value = parse(bytes)?;
    if value["format"] != "oddlang-provider-diagnostics-v1" {
        return serde_json::from_value(value).map_err(|e| e.to_string());
    }
    let failure: Failure = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if failure.format != "oddlang-provider-diagnostics-v1"
        || failure.diagnostics.is_empty()
        || failure.diagnostics.len() > FILE_LIMIT
    {
        return Err("invalid provider diagnostic document".into());
    }
    let mut messages = Vec::new();
    for item in failure.diagnostics {
        if !identifier(&item.code)
            || item.message.is_empty()
            || item.message.len() > 4096
            || item.message.chars().any(char::is_control)
        {
            return Err("invalid provider diagnostic".into());
        }
        let path = item.path.as_deref().unwrap_or("");
        if item.path.is_some() {
            logical_path(path)?;
        }
        messages.push(format!("{} {path}: {}", item.code, item.message));
    }
    Err(messages.join("\n"))
}

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:/_-+".contains(&b))
}
fn selection(role: &str, capability: &str, version: &str) -> Result<()> {
    if !matches!(role, "recover" | "generate") || !identifier(capability) || !identifier(version) {
        return Err("invalid provider role, capability or version".into());
    }
    Ok(())
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        Self::read_registry(path, None)
    }

    pub fn load_pinned(path: &Path, sha256: &str) -> Result<Self> {
        Self::read_registry(path, Some(sha256))
    }

    fn read_registry(path: &Path, expected: Option<&str>) -> Result<Self> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let base = path.parent().ok_or("missing provider registry directory")?;
        let bytes = read_file(&path, MIB)?;
        if expected.is_some_and(|pin| digest(&bytes) != pin) {
            return Err("workspace provider registry digest mismatch".into());
        }
        let manifest: Manifest = parse(&bytes)?;
        if manifest.format != "oddlang-provider-registry-v1" || manifest.providers.len() > 64 {
            return Err("unsupported or oversized provider registry".into());
        }
        let mut registry = Self::default();
        let mut total = 0;
        for registration in manifest.providers {
            let r = &registration;
            selection(&r.role, &r.capability, &r.version)?;
            if r.world != format!("oddlang:provider/{}@1.0.0", r.role) {
                return Err("incompatible provider world".into());
            }
            let key = (r.role.clone(), r.capability.clone(), r.version.clone());
            if registry.entries.contains_key(&key) {
                return Err("ambiguous provider registration".into());
            }
            let schema: schema::Schema = parse(&r.schema.read(base, 64 * 1024)?)?;
            schema.check(0)?;
            let component = r.component.read(base, MAX_COMPONENT_BYTES)?;
            total += component.len();
            if total > 64 * MIB {
                return Err("provider registry byte limit".into());
            }
            registry.entries.insert(
                key,
                External {
                    registration,
                    component,
                    schema,
                },
            );
        }
        Ok(registry)
    }

    pub fn run(
        &self,
        role: &str,
        capability: &str,
        version: &str,
        input: Value,
        allowed_dependencies: &[String],
        builtin: impl FnOnce() -> Result<ProviderOutput>,
    ) -> Result<(ProviderOutput, Value)> {
        selection(role, capability, version)?;
        let key = (role.to_owned(), capability.to_owned(), version.to_owned());
        let external = self.entries.get(&key);
        if external.is_none()
            && self
                .entries
                .keys()
                .any(|(r, c, _)| r == role && c == capability)
        {
            return Err("incompatible provider contract version".into());
        }
        let config = external
            .map(|e| &e.registration.configuration)
            .unwrap_or(&Value::Null);
        let request = serde_json::to_vec(&json!({"format":"oddlang-provider-input-v1","role":role,
            "capability":capability,"version":version,"configuration":config,"input":input}))
        .map_err(|e| e.to_string())?;
        if request.len() > DOCUMENT_LIMIT {
            return Err("provider request byte limit".into());
        }
        let (output, identity) = if let Some(ext) = external {
            ext.schema.validate(&input)?;
            let raw = crate::provider_runtime::invoke(role, &ext.component, &request)?;
            if raw.len() > DOCUMENT_LIMIT {
                return Err("provider response byte limit".into());
            }
            let response = decode_response(&raw)?;
            if response.format != "oddlang-provider-output-v1"
                || response.artifacts.len() > FILE_LIMIT
            {
                return Err("invalid provider output envelope".into());
            }
            let mut artifacts = BTreeMap::new();
            for a in response.artifacts {
                let bytes = STANDARD.decode(a.base64).map_err(|e| e.to_string())?;
                if artifacts.insert(a.path, bytes).is_some() {
                    return Err("duplicate provider artifact".into());
                }
            }
            (
                ProviderOutput {
                    artifacts,
                    exports: response.exports,
                    dependencies: response.dependencies,
                    evidence: response.evidence,
                    gaps: response.gaps,
                },
                json!({"world":ext.registration.world,"componentSha256":ext.registration.component.sha256,
                    "schemaSha256":ext.registration.schema.sha256,"configuration":config}),
            )
        } else {
            (
                builtin()?,
                json!({"nativeRelease":env!("CARGO_PKG_VERSION")}),
            )
        };
        output.validate(allowed_dependencies)?;
        let files: BTreeMap<_, _> = output
            .artifacts
            .iter()
            .map(|(name, b)| (name, digest(b)))
            .collect();
        let receipt = json!({"format":"oddlang-provider-receipt-v1","role":role,"capability":capability,
            "version":version,"provider":identity,"requestSha256":digest(&request),"files":files,
            "dependencies":output.dependencies,"exports":output.exports,"evidence":output.evidence,"gaps":output.gaps});
        Ok((output, receipt))
    }
}

pub fn read_input(path: &Path) -> Result<Vec<u8>> {
    read_file(path, DOCUMENT_LIMIT)
}

impl ProviderOutput {
    fn validate_paths(&self) -> Result<()> {
        let mut nodes = BTreeMap::new();
        for name in self.artifacts.keys().map(String::as_str).chain([RECEIPT]) {
            logical_path(name)?;
            let mut prefix = String::new();
            for part in name.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                let directory = prefix != name;
                let key = prefix.to_ascii_lowercase();
                if let Some((previous, is_dir)) = nodes.insert(key, (prefix.clone(), directory))
                    && (previous != prefix || !is_dir || !directory)
                {
                    return Err(format!("colliding provider path {name}"));
                }
            }
        }
        Ok(())
    }

    fn validate(&self, allowed: &[String]) -> Result<()> {
        if self.artifacts.len() > FILE_LIMIT
            || self.artifacts.values().map(Vec::len).sum::<usize>() > DOCUMENT_LIMIT
        {
            return Err("provider artifact limit".into());
        }
        self.validate_paths()?;
        for names in [&self.exports, &self.dependencies] {
            let mut seen = BTreeSet::new();
            if names.len() > FILE_LIMIT || names.iter().any(|n| !identifier(n) || !seen.insert(n)) {
                return Err("invalid or duplicate provider declaration".into());
            }
        }
        if self.dependencies.iter().any(|d| !allowed.contains(d)) {
            return Err("unsupported provider dependency".into());
        }
        for list in [&self.evidence, &self.gaps] {
            if list.len() > FILE_LIMIT || list.iter().any(|s| s.is_empty() || s.len() > 4096) {
                return Err("invalid provider evidence or gap".into());
            }
        }
        Ok(())
    }
}

pub fn publish(dest: &Path, output: &ProviderOutput, receipt: &Value) -> Result<()> {
    output.validate_paths()?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    for (name, bytes) in &output.artifacts {
        let path = staging.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    fs::write(
        staging.path().join(RECEIPT),
        serde_json::to_vec_pretty(receipt).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, staging.path(), CWD, dest, RenameFlags::NOREPLACE)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err("atomic provider publication unavailable on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collisions_leave_destination_absent() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("out");
        for (a, b) in [
            ("a/b", "A/c"),
            ("foo", "foo/bar"),
            ("A.rs", "a.rs"),
            ("provider-receipt.json", "ok"),
        ] {
            let mut out = ProviderOutput::default();
            out.artifacts.insert(a.into(), vec![]);
            out.artifacts.insert(b.into(), vec![]);
            assert!(publish(&dest, &out, &Value::Null).is_err());
            assert!(!dest.exists());
        }
    }
    #[test]
    fn native_and_external_share_policy_and_receipt() {
        let registry = Registry::default();
        let run = |approved: &[String]| {
            registry.run("generate", "example", "1", json!({"x":1}), approved, || {
                Ok(ProviderOutput {
                    dependencies: vec!["serde".into()],
                    artifacts: BTreeMap::from([("a/b".into(), vec![0, 255])]),
                    ..Default::default()
                })
            })
        };
        assert!(run(&[]).is_err());
        let (out, receipt) = run(&["serde".into()]).unwrap();
        assert_eq!(receipt["files"]["a/b"], digest(&[0, 255]));
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("out");
        publish(&dest, &out, &receipt).unwrap();
        assert_eq!(fs::read(dest.join("a/b")).unwrap(), [0, 255]);
        assert!(publish(&dest, &out, &receipt).is_err());
    }
    #[test]
    fn pinned_schema_and_version_are_enforced_before_invocation() {
        let root = tempfile::tempdir().unwrap();
        let pin = |name: &str, bytes: &[u8]| {
            fs::write(root.path().join(name), bytes).unwrap();
            json!({"path":name,"sha256":digest(bytes)})
        };
        let registration = json!({"role":"generate","capability":"experimental","version":"1",
            "world":"oddlang:provider/generate@1.0.0","configuration":{},
            "component":pin("example.wasm",b"invalid"),"schema":pin("schema.json",br#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#)});
        let path = root.path().join("registry.json");
        let write = |rows: Value| {
            fs::write(
                &path,
                json!({"format":"oddlang-provider-registry-v1","providers":rows}).to_string(),
            )
            .unwrap()
        };
        write(json!([registration]));
        let registry = Registry::load(&path).unwrap();
        let run = |version| {
            registry
                .run(
                    "generate",
                    "experimental",
                    version,
                    json!({}),
                    &[],
                    || panic!(),
                )
                .unwrap_err()
        };
        assert!(run("2").contains("incompatible"));
        assert!(run("1").contains("missing provider property"));
        write(json!([registration, registration]));
        assert!(Registry::load(&path).is_err());
        write(json!([registration]));
        fs::write(root.path().join("schema.json"), b"{}").unwrap();
        assert!(Registry::load(&path).is_err());
    }
}

/// Decode a host-selected document without accepting shadowed JSON keys.
pub fn document(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > DOCUMENT_LIMIT {
        return Err("provider document byte limit".into());
    }
    parse(bytes)
}
