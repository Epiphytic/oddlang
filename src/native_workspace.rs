//! Build native workspace members from individually pinned semantic package inputs.
use crate::native_package::{self, Pin, digest};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};
type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Workspace {
    format: String,
    cargo_lock: Pin,
    members: BTreeMap<String, Member>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Member {
    manifest: Pin,
    #[serde(default)]
    dependencies: BTreeSet<String>,
}
fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| e.to_string())
}
fn write(path: &Path, data: impl AsRef<[u8]>) -> Result<()> {
    fs::write(path, data).map_err(|e| e.to_string())
}

pub fn build(path: &Path, destination: &Path) -> Result<()> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let base = path.parent().ok_or("missing workspace directory")?;
    let raw = read(&path)?;
    let workspace: Workspace = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if workspace.format != "oddlang-native-workspace-v1" || workspace.members.is_empty() {
        return Err("invalid or empty native workspace".into());
    }
    if destination.symlink_metadata().is_ok() {
        return Err("workspace destination already exists".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    let root = stage.path().join("workspace");
    fs::create_dir(&root).map_err(|e| e.to_string())?;
    let mut records = BTreeMap::new();
    let mut members = vec![];
    let mut has_binaries = false;
    for (name, member) in &workspace.members {
        if name.eq_ignore_ascii_case("target")
            || !name.bytes().next().is_some_and(|c| c.is_ascii_alphabetic())
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return Err(format!("invalid member name {name:?}"));
        }
        let data = member.manifest.read(base)?;
        let spec: serde_json::Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        if spec["name"] != name.as_str() {
            return Err(format!("member identity differs from package: {name}"));
        }
        has_binaries |= spec["binaries"].as_array().is_some_and(|b| !b.is_empty());
        let member_base = base
            .join(&member.manifest.path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let dir = root.join(name);
        native_package::build_pinned(&data, member_base.parent().unwrap(), &dir, false)?;
        let cargo_file = dir.join("Cargo.toml");
        let mut cargo: toml::Table = std::str::from_utf8(&read(&cargo_file)?)
            .map_err(|e| e.to_string())?
            .parse::<toml::Table>()
            .map_err(|e| e.to_string())?;
        cargo.remove("workspace");
        let deps = cargo
            .get_mut("dependencies")
            .and_then(toml::Value::as_table_mut)
            .ok_or("missing generated dependencies")?;
        for dependency in &member.dependencies {
            if dependency == name || !workspace.members.contains_key(dependency) {
                return Err(format!(
                    "unresolved or self workspace edge {name} -> {dependency}"
                ));
            }
            if deps
                .insert(
                    dependency.clone(),
                    toml::Value::Table(toml::Table::from_iter([(
                        "path".into(),
                        toml::Value::String(format!("../{dependency}")),
                    )])),
                )
                .is_some()
            {
                return Err(format!(
                    "workspace edge collides with external dependency: {dependency}"
                ));
            }
        }
        let assembled = toml::to_string_pretty(&cargo).map_err(|e| e.to_string())?;
        write(&cargo_file, &assembled)?;
        fs::remove_file(dir.join("Cargo.lock")).map_err(|e| e.to_string())?;
        let package_lock = dir.join("package.lock");
        records.insert(name,json!({"inputSha256":digest(&data),"standalonePackageLockSha256":digest(&read(&package_lock)?),
            "workspaceCargoManifestSha256":digest(assembled.as_bytes()),"dependencies":member.dependencies}));
        members.push(name);
    }
    let lock = workspace.cargo_lock.read(base)?;
    write(&root.join("Cargo.lock"), &lock)?;
    let manifest = toml::to_string_pretty(&json!({"workspace":{"resolver":"2","members":members}}))
        .map_err(|e| e.to_string())?;
    write(&root.join("Cargo.toml"), &manifest)?;
    let result = Command::new("cargo")
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "invalid composed workspace: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    if has_binaries {
        let checked = Command::new("cargo")
            .args([
                "check",
                "--workspace",
                "--bins",
                "--offline",
                "--locked",
                "--target-dir",
            ])
            .arg(stage.path().join("bin-check"))
            .current_dir(&root)
            .output()
            .map_err(|e| e.to_string())?;
        if !checked.status.success() {
            return Err(format!(
                "invalid workspace binary entry binding: {}",
                String::from_utf8_lossy(&checked.stderr)
            ));
        }
    }
    let receipt = json!({"format":"oddlang-native-workspace-lock-v1","inputSha256":digest(&raw),"workspaceCargoManifestSha256":digest(manifest.as_bytes()),"cargoLockSha256":digest(&lock),"members":records});
    write(
        &root.join("workspace.lock"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    )?;
    native_package::publish(&root, destination)
}
