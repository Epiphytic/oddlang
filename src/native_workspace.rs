//! Build native workspace members from individually pinned semantic package inputs.
use crate::native_package::{self, DependencyCache, Pin, digest};
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

fn merge_registry_patches(target: &mut toml::Table, patches: toml::Table) -> Result<()> {
    for (crate_name, patch) in patches {
        match target.entry(crate_name) {
            toml::map::Entry::Vacant(slot) => {
                slot.insert(patch);
            }
            toml::map::Entry::Occupied(slot) if slot.get() != &patch => {
                return Err(format!("conflicting crates.io patch: {}", slot.key()));
            }
            toml::map::Entry::Occupied(_) => {}
        }
    }
    Ok(())
}

pub fn build(path: &Path, destination: &Path) -> Result<()> {
    assemble(path, destination, DependencyCache::Offline)
}

pub fn build_fetch_locked(path: &Path, destination: &Path) -> Result<()> {
    assemble(path, destination, DependencyCache::FetchLocked)
}

fn assemble(path: &Path, destination: &Path, cache: DependencyCache) -> Result<()> {
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
    let mut providers = std::collections::BTreeSet::new();
    let mut registry_patches = toml::Table::new();
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
        providers.extend(native_package::build_pinned(
            &data,
            member_base.parent().unwrap(),
            &dir,
            Some(&member.dependencies),
            cache,
        )?);
        let cargo_file = dir.join("Cargo.toml");
        let mut cargo: toml::Table = std::str::from_utf8(&read(&cargo_file)?)
            .map_err(|e| e.to_string())?
            .parse::<toml::Table>()
            .map_err(|e| e.to_string())?;
        cargo.remove("workspace");
        if let Some(patch) = cargo.remove("patch") {
            let selected = patch
                .get("crates-io")
                .and_then(toml::Value::as_table)
                .ok_or("missing generated crates.io patches")?;
            merge_registry_patches(&mut registry_patches, selected.clone())?;
        }
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
    validate_registry_providers(&providers, &registry_patches, &lock)?;
    write(&root.join("Cargo.lock"), &lock)?;
    let mut manifest = json!({"workspace":{"resolver":"2","members":members}});
    if !registry_patches.is_empty() {
        manifest["patch"] = json!({"crates-io": registry_patches});
    }
    let manifest = toml::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write(&root.join("Cargo.toml"), &manifest)?;
    cache.resolve(&root, "invalid composed workspace")?;
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

fn validate_registry_providers(
    providers: &std::collections::BTreeSet<native_package::RegistryProvider>,
    registry_patches: &toml::Table,
    lock: &[u8],
) -> Result<()> {
    for provider in providers {
        if registry_patches.contains_key(provider.name) {
            return Err(format!(
                "workspace patch overrides required registry provider {}",
                provider.name
            ));
        }
        provider.verify_lock(lock)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_patch_cannot_replace_the_fixed_byte_provider() {
        let mut merged = toml::Table::new();
        merge_registry_patches(
            &mut merged,
            "[zeroize]\ngit='https://example.org/provider'"
                .parse()
                .unwrap(),
        )
        .unwrap();
        let required = [native_package::ZEROIZE].into_iter().collect();
        assert!(
            validate_registry_providers(&required, &merged, b"")
                .unwrap_err()
                .contains("patch overrides")
        );
        assert!(
            validate_registry_providers(&required, &toml::Table::new(), b"package=[]")
                .unwrap_err()
                .contains("expected locked registry")
        );
    }

    #[test]
    fn workspace_accepts_agreeing_registry_pins_and_rejects_conflicts() {
        let a: toml::Table = r#"
[packet-storage]
git = "https://example.org/packet-storage"
rev = "1111111111111111111111111111111111111111"
"#
        .parse()
        .unwrap();
        let mut merged = toml::Table::new();
        merge_registry_patches(&mut merged, a.clone()).unwrap();
        merge_registry_patches(&mut merged, a.clone()).unwrap();
        assert_eq!(merged, a);
        let b = toml::to_string(&a)
            .unwrap()
            .replace(
                "1111111111111111111111111111111111111111",
                "2222222222222222222222222222222222222222",
            )
            .parse()
            .unwrap();
        assert_eq!(
            merge_registry_patches(&mut merged, b).unwrap_err(),
            "conflicting crates.io patch: packet-storage"
        );
    }
}
