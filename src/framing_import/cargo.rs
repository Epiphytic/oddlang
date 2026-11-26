//! Resolve in the selected workspace and compare compiler artifacts to the locked primitive profile.
use super::{Result, hash, read};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::{Path, PathBuf},
    process::Command,
};
use syn::parse_quote;

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
const COMMIT: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";
const CFG_HASH: &str = "2d39557b91c89de5c7790a53e35fd76f3ab94fbb5a8df43ed348da749cc81140";
const PRIMITIVES: [(&str, &[&str]); 7] = [
    ("base64@0.22.1", &["alloc", "default", "std"]),
    (
        "serde@1.0.229",
        &["default", "derive", "serde_derive", "std"],
    ),
    ("serde_core@1.0.229", &["result", "std"]),
    ("serde_derive@1.0.229", &["default"]),
    ("serde_json@1.0.151", &["default", "std"]),
    ("thiserror@2.0.21", &["default", "std"]),
    ("thiserror-impl@2.0.21", &[]),
];
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Build {
    compiler: String,
    target: String,
    package: String,
    edition: String,
    flags: Vec<String>,
    pub(super) inputs: BTreeMap<String, String>,
    workspace_manifest_sha256: String,
    cargo_lock_sha256: String,
    compiled_features: BTreeMap<String, Vec<String>>,
    registry_checksums: BTreeMap<String, String>,
}
impl Build {
    pub(super) fn validate(&self) -> Result<()> {
        let approved: BTreeMap<String, String> =
            serde_json::from_str(include_str!("registry.json")).unwrap();
        if !self
            .compiler
            .lines()
            .any(|l| l == format!("commit-hash: {COMMIT}"))
            || !self
                .compiler
                .lines()
                .any(|l| l == format!("host: {}", self.target))
            || self.edition != "2021"
            || self.flags != ["-C", "overflow-checks=yes"]
            || self
                .registry_checksums
                .keys()
                .ne(self.compiled_features.keys())
            || self
                .registry_checksums
                .iter()
                .any(|(k, v)| approved.get(k) != Some(v))
        {
            return Err("invalid framing compiler or dependency provenance".into());
        }
        let names: Vec<_> = self.inputs.keys().map(String::as_str).collect();
        if !(names == ["Cargo.toml", "src/frame.rs", "src/lib.rs"]
            || names == ["Cargo.toml", "src/config.rs", "src/frame.rs", "src/lib.rs"])
            || self.inputs.values().any(|h| !super::valid_digest(h))
            || !super::valid_digest(&self.workspace_manifest_sha256)
            || !super::valid_digest(&self.cargo_lock_sha256)
            || self.package.is_empty()
            || self.package.len() > 64
            || !self
                .package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("invalid framing build input identity".into());
        }
        for (name, features) in PRIMITIVES {
            if !self
                .compiled_features
                .get(name)
                .is_some_and(|f| f == features)
            {
                return Err(format!(
                    "unexpected primitive package or enabled features: {name}"
                ));
            }
        }
        Ok(())
    }
}
fn command(cmd: &mut Command) -> Result<String> {
    let o = cmd.output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).into_owned());
    }
    String::from_utf8(o.stdout).map_err(|e| e.to_string())
}
fn crate_root(root: &Path) -> Result<BTreeMap<String, String>> {
    let bytes = read(&root.join("src/lib.rs"))?;
    let file = syn::parse_file(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if file.shebang.is_some() || file.attrs.iter().any(|a| !super::syntax::literal_doc(a)) {
        return Err("unsupported crate-root attributes".into());
    }
    let mut seen = BTreeSet::new();
    for item in file.items {
        let label = if item
            == parse_quote!(
                mod frame;
            ) {
            "frame"
        } else if item
            == parse_quote!(
                pub use frame::{FrameError, Limits, ProbeEnvelope};
            )
        {
            "exports"
        } else if item
            == parse_quote!(
                mod config;
            )
        {
            "config"
        } else if item
            == parse_quote!(
                pub use config::{ConfigError, RelaySet, Retention};
            )
        {
            "config exports"
        } else {
            return Err("unsupported crate-root declaration or shadowing".into());
        };
        if !seen.insert(label) {
            return Err("duplicate crate-root declaration".into());
        }
    }
    if !seen.contains("frame")
        || !seen.contains("exports")
        || seen.contains("config") != seen.contains("config exports")
    {
        return Err("invalid public framing module closure".into());
    }
    let mut inputs = BTreeMap::from([("src/lib.rs".into(), hash(&bytes))]);
    if seen.contains("config") {
        let digest = hash(&read(&root.join("src/config.rs"))?);
        if digest != CFG_HASH {
            return Err("unrecognized retained sibling of framing module".into());
        }
        inputs.insert("src/config.rs".into(), digest);
    }
    Ok(inputs)
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| format!("missing Cargo {k}"))
}
fn rows<'a>(v: &'a Value, k: &str) -> Result<&'a Vec<Value>> {
    v[k].as_array().ok_or_else(|| format!("missing Cargo {k}"))
}
pub(super) fn check(manifest: &Path, package: &str, frame: &[u8]) -> Result<Build> {
    let root = manifest.parent().ok_or("missing crate directory")?;
    let source = root.join("src");
    if source.canonicalize().map_err(|e| e.to_string())? != source {
        return Err("redirected framing source directory".into());
    }
    let mut inputs = crate_root(root)?;
    inputs.insert("src/frame.rs".into(), hash(frame));
    inputs.insert("Cargo.toml".into(), hash(&read(manifest)?));
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let rustup = |tool: &str| {
        command(Command::new("rustup").args(["which", "--toolchain", "1.97.1", tool]))
            .map(|s| PathBuf::from(s.trim()))
    };
    let rustc = rustup("rustc")?;
    let cargo = rustup("cargo")?;
    let compiler = command(Command::new(&rustc).arg("-vV"))?;
    let target = compiler
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("rustc missing host")?
        .to_owned();
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".cargo")))
        .ok_or("missing Cargo home")?;
    for name in ["config", "config.toml"] {
        let path = cargo_home.join(name);
        if path.exists() {
            let config: toml::Table =
                toml::from_str(std::str::from_utf8(&read(&path)?).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if config.iter().any(|(k, v)| {
                k != "target"
                    || v.as_table().is_none_or(|t| {
                        t.keys().any(|k| {
                            k == &target
                                || !k
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                        })
                    })
            }) {
                return Err("unsupported Cargo host configuration for framing-v1".into());
            }
        }
    }
    if scratch.path().ancestors().any(|p| {
        [".cargo/config", ".cargo/config.toml"]
            .iter()
            .any(|c| p.join(c).exists())
    }) {
        return Err("unsupported Cargo configuration in compiler working directory".into());
    }
    let cmd = || {
        let mut c = Command::new(&cargo);
        c.current_dir(scratch.path()).env_clear();
        for key in [
            "HOME",
            "PATH",
            "CARGO_HOME",
            "TMPDIR",
            "SYSTEMROOT",
            "SDKROOT",
            "DEVELOPER_DIR",
        ] {
            if let Some(v) = env::var_os(key) {
                c.env(key, v);
            }
        }
        c.env("CARGO_NET_OFFLINE", "true")
            .env("RUSTC", &rustc)
            .env("CARGO_TARGET_DIR", scratch.path().join("target"));
        c
    };
    let metadata: Value = serde_json::from_str(&command(
        cmd()
            .args([
                "metadata",
                "--locked",
                "--offline",
                "--format-version",
                "1",
                "--filter-platform",
                &target,
                "--manifest-path",
            ])
            .arg(manifest),
    )?)
    .map_err(|e| e.to_string())?;
    let ws = Path::new(text(&metadata, "workspace_root")?);
    let workspace_manifest = read(&ws.join("Cargo.toml"))?;
    let workspace_manifest_sha256 = hash(&workspace_manifest);
    let lock = read(&ws.join("Cargo.lock"))?;
    let cargo_lock_sha256 = hash(&lock);
    // The pinned workspace uses only its audited build-profile settings. Reject
    // workspace patches/profile overrides in other workspaces before compilation.
    let workspace: toml::Table =
        toml::from_str(std::str::from_utf8(&workspace_manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if (workspace.contains_key("profile")
        || workspace.contains_key("patch")
        || workspace.contains_key("replace"))
        && workspace_manifest_sha256
            != "bf726da78321c6f465902f679bbd05557374091458208fe5531be8b3cc2e4f53"
    {
        return Err("unrecognized workspace profile or dependency override".into());
    }
    for p in root.ancestors().take_while(|p| p.starts_with(ws)) {
        if [".cargo/config", ".cargo/config.toml"]
            .iter()
            .any(|f| p.join(f).exists())
        {
            return Err("unsupported local Cargo configuration".into());
        }
    }
    let packages: BTreeMap<_, _> = rows(&metadata, "packages")?
        .iter()
        .map(|p| Ok((text(p, "id")?, p)))
        .collect::<Result<_>>()?;
    let selected = packages
        .values()
        .find(|p| p["manifest_path"].as_str() == manifest.to_str())
        .ok_or("selected Cargo manifest missing")?;
    if text(selected, "name")? != package
        || selected["edition"] != "2021"
        || selected["features"]
            .as_object()
            .is_none_or(|v| !v.is_empty())
    {
        return Err("unexpected framing package identity, edition or features".into());
    }
    for t in rows(selected, "targets")? {
        if t["kind"] == serde_json::json!(["custom-build"]) {
            return Err("framing crate build scripts are unsupported".into());
        }
    }
    if !rows(selected, "targets")?.iter().any(|t| {
        t["kind"] == serde_json::json!(["lib"])
            && t["src_path"].as_str() == root.join("src/lib.rs").to_str()
    }) {
        return Err("unsupported framing library root".into());
    }
    for d in rows(selected, "dependencies")? {
        let name = text(d, "name")?;
        if !["base64", "serde", "serde_json", "thiserror", "url"].contains(&name)
            || d["source"] != REGISTRY
            || !d["target"].is_null()
            || !d["rename"].is_null()
            || !d["kind"].is_null()
            || d["optional"] != false
            || d["uses_default_features"] != true
            || d["features"]
                != if name == "serde" {
                    serde_json::json!(["derive"])
                } else {
                    serde_json::json!([])
                }
        {
            return Err("unsupported framing dependency, alias or features".into());
        }
    }
    let locks: toml::Table = toml::from_str(std::str::from_utf8(&lock).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let checksums: BTreeMap<_, _> = locks["package"]
        .as_array()
        .ok_or("invalid Cargo.lock")?
        .iter()
        .filter_map(|p| {
            Some((
                format!(
                    "{}@{}",
                    p.get("name")?.as_str()?,
                    p.get("version")?.as_str()?
                ),
                p.get("checksum")?.as_str()?.to_owned(),
            ))
        })
        .collect();
    let approved: BTreeMap<String, String> =
        serde_json::from_str(include_str!("registry.json")).unwrap();
    let nodes: BTreeMap<_, _> = rows(&metadata["resolve"], "nodes")?
        .iter()
        .map(|n| Ok((text(n, "id")?, n)))
        .collect::<Result<_>>()?;
    let mut pending = vec![text(selected, "id")?];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if id != text(selected, "id")? {
            let p = packages.get(id).ok_or("unknown Cargo dependency")?;
            let key = format!("{}@{}", text(p, "name")?, text(p, "version")?);
            if p["source"] != REGISTRY
                || !approved.contains_key(&key)
                || approved.get(&key) != checksums.get(&key)
            {
                return Err(format!("unrecognized resolved dependency: {key}"));
            }
        }
        for d in rows(nodes.get(id).ok_or("missing resolve node")?, "deps")? {
            if rows(d, "dep_kinds")?
                .iter()
                .any(|k| k["kind"].is_null() || k["kind"] == "build")
            {
                pending.push(text(d, "pkg")?);
            }
        }
    }
    let output = command(
        cmd()
            .args([
                "rustc",
                "--locked",
                "--offline",
                "--lib",
                "--message-format=json",
                "--manifest-path",
            ])
            .arg(manifest)
            .args(["-p", package, "--", "-C", "overflow-checks=yes"]),
    )?;
    let mut compiled_features = BTreeMap::new();
    let mut registry_checksums = BTreeMap::new();
    for line in output.lines() {
        let event: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if event["reason"] != "compiler-artifact" {
            continue;
        }
        let id = text(&event, "package_id")?;
        if id == text(selected, "id")? {
            continue;
        }
        let p = packages.get(id).ok_or("unknown compiler artifact")?;
        let key = format!("{}@{}", text(p, "name")?, text(p, "version")?);
        if !seen.contains(id) {
            return Err("compiler artifact outside resolved closure".into());
        }
        let features: Vec<String> =
            serde_json::from_value(event["features"].clone()).map_err(|e| e.to_string())?;
        if compiled_features
            .insert(key.clone(), features.clone())
            .is_some_and(|prior| prior != features)
        {
            return Err(format!("ambiguous build-unit features for {key}"));
        }
        registry_checksums.insert(key.clone(), checksums[&key].clone());
    }
    for (path, sha) in &inputs {
        if &hash(&read(&root.join(path))?) != sha {
            return Err("source changed during compilation".into());
        }
    }
    if cargo_lock_sha256 != hash(&read(&ws.join("Cargo.lock"))?)
        || workspace_manifest_sha256 != hash(&read(&ws.join("Cargo.toml"))?)
    {
        return Err("workspace changed during compilation".into());
    }
    let record = Build {
        compiler,
        target,
        package: package.into(),
        edition: "2021".into(),
        flags: vec!["-C".into(), "overflow-checks=yes".into()],
        inputs,
        workspace_manifest_sha256,
        cargo_lock_sha256,
        compiled_features,
        registry_checksums,
    };
    record.validate()?;
    Ok(record)
}
