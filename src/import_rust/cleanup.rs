//! Verify the reviewed patch and its two archived inputs during offline import.
use super::{Res, digest, read};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u32,
    manifest: String,
    mapping: String,
    original_tree_sha256: String,
    effective_tree_sha256: String,
    patch_sha256: String,
}

fn entries(root: &Path, dir: &Path, result: &mut BTreeMap<String, String>) -> Res<()> {
    if !fs::symlink_metadata(dir)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("expected cleanup snapshot directory".into());
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .ok_or("non-UTF-8 cleanup path")?;
        if matches!(name, "target" | ".git") {
            continue;
        }
        if !crate::contracts::normal(name) {
            return Err("invalid cleanup path".into());
        }
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            entries(root, &path, result)?;
        } else {
            result.insert(name.to_owned(), digest(&read(&path)?));
        }
    }
    Ok(())
}

fn snapshot(root: &Path, expected: &str) -> Res<BTreeMap<String, String>> {
    let mut entries_map = BTreeMap::new();
    entries(root, root, &mut entries_map)?;
    let bytes: String = entries_map
        .iter()
        .map(|(name, hash)| format!("{name}\0{hash}\n"))
        .collect();
    if digest(bytes.as_bytes()) != expected {
        return Err("cleanup snapshot digest mismatch".into());
    }
    Ok(entries_map)
}

pub fn verify(receipt: &Path, manifest: &Path, mapping: &Path) -> Res<serde_json::Value> {
    if receipt.file_name().and_then(|n| n.to_str()) != Some("accepted.json") {
        return Err("cleanup requires accepted.json".into());
    }
    let root = receipt.parent().ok_or("missing cleanup archive")?;
    let record: Receipt = serde_json::from_slice(&read(receipt)?).map_err(|e| e.to_string())?;
    if record.version != 1 {
        return Err("unknown cleanup format".into());
    }
    let original = root.join("original");
    let effective = root.join("effective");
    for (file, name) in [(manifest, &record.manifest), (mapping, &record.mapping)] {
        if !crate::contracts::normal(name)
            || file.canonicalize().map_err(|e| e.to_string())?
                != effective
                    .join(name)
                    .canonicalize()
                    .map_err(|e| e.to_string())?
        {
            return Err("cleanup receipt names different import inputs".into());
        }
    }
    let before = snapshot(&original, &record.original_tree_sha256)?;
    let after = snapshot(&effective, &record.effective_tree_sha256)?;
    if before == after
        || before.keys().ne(after.keys())
        || before.iter().any(|(name, hash)| {
            after.get(name) != Some(hash) && !(name.ends_with(".rs") || name == &record.mapping)
        })
    {
        return Err("expected a source or mapping cleanup".into());
    }
    let patch = root.join("cleanup.patch");
    if digest(&read(&patch)?) != record.patch_sha256 {
        return Err("cleanup patch digest mismatch".into());
    }
    let replay = tempfile::tempdir().map_err(|e| e.to_string())?;
    for name in before.keys() {
        let target = replay.path().join(name);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(original.join(name), target).map_err(|e| e.to_string())?;
    }
    // --no-index keeps the patch away from a caller's index and working tree.
    let output = Command::new("git")
        .current_dir(replay.path())
        .args(["apply", "--no-index", "--whitespace=nowarn", "-p2", "--"])
        .arg(patch.canonicalize().map_err(|e| e.to_string())?)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("cannot replay accepted cleanup patch".into());
    }
    snapshot(replay.path(), &record.effective_tree_sha256)?;
    serde_json::to_value(record).map_err(|e| e.to_string())
}
