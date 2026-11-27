//! Observe owned entries and test the original's lock in the platform's lock space.
use anyhow::{Result, bail, ensure};
use marmish_inventory::{Map, array, digest, object, text};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, os::unix::fs::MetadataExt, path::Path};
pub type Hashes = BTreeMap<String, String>;

fn walk(root: &Path, dir: &Path, entries: &mut Map, hashes: &mut Hashes) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid state path"))?;
        let m = fs::symlink_metadata(&path)?;
        ensure!(
            m.uid() == unsafe { libc::geteuid() },
            "unexpected owner: {name}"
        );
        let mode = format!("{:04o}", m.mode() & 0o7777);
        let observation = if m.is_dir() {
            walk(root, &path, entries, hashes)?;
            json!({"type":"dir","mode":mode})
        } else if m.is_file() {
            let bytes = fs::read(&path)?;
            let mut obs = json!({"type":"file","mode":mode,"bytes":m.len()});
            if name == "state/engine.key" {
                obs["magic"] = json!(super::latin1(&bytes[..bytes.len().min(8)]));
            }
            hashes.insert(name.into(), digest(&bytes));
            obs
        } else {
            bail!("unexpected filesystem entry {name}");
        };
        entries.insert(name.into(), observation);
    }
    Ok(())
}
pub fn inspect(root: &Path) -> Result<(Value, Hashes)> {
    let mut entries = Map::new();
    let mut hashes = Hashes::new();
    walk(root, root, &mut entries, &mut hashes)?;
    let lock = if entries.contains_key("state/frontend.lock") {
        super::locks::observe(&root.join("state/frontend.lock"))?
    } else {
        "absent"
    };
    Ok((json!({"entries":entries,"frontend_lock":lock}), hashes))
}
pub fn compare(expected: &Value, actual: &Value) -> Result<()> {
    ensure!(
        expected["frontend_lock"] == actual["frontend_lock"],
        "frontend lock differs"
    );
    let entries = object(&expected["entries"])?;
    let got = object(&actual["entries"])?;
    ensure!(
        entries.len() == got.len() && entries.keys().all(|p| got.contains_key(p)),
        "state membership differs"
    );
    for (path, shape) in entries {
        let fields = array(shape)?;
        let observed = &got[path];
        ensure!(
            fields.len() >= 2 && fields[0] == observed["type"] && fields[1] == observed["mode"],
            "entry type or mode differs: {path}"
        );
        if let Some(contents) = fields.get(2) {
            let ok = match text(contents)? {
                "key40" => observed["bytes"] == 40 && observed["magic"] == "MRMSKEY1",
                "empty" => observed["bytes"] == 0,
                "nonempty" => observed["bytes"].as_u64().is_some_and(|v| v > 0),
                _ => bail!("unknown state expectation"),
            };
            ensure!(ok, "entry contents differ: {path}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_state_rejects_unlocked_file_mode_and_key_drift() -> Result<()> {
        let root = marmish_inventory::root().join("examples/marmish/portable-bridge");
        let baseline = marmish_inventory::read_json(&root.join("baseline.json"))?;
        let archive = marmish_inventory::read_json(&root.join("original.json"))?;
        let live = &baseline["states"]["live"];
        let row = array(&archive["cases"])?
            .iter()
            .find(|r| r["id"] == "version-1-init-before-eof")
            .unwrap();
        let observed = &row["exchanges"][0]["state"];
        compare(live, observed)?;
        for (pointer, change) in [
            ("/frontend_lock", json!("available")),
            ("/entries/state~1engine.key/mode", json!("0644")),
            ("/entries/state~1engine.key/bytes", json!(39)),
            ("/entries/state~1engine.key/magic", json!("incorrect")),
            ("/entries/state~1engine.sqlite3-wal/bytes", json!(0)),
        ] {
            let mut altered = observed.clone();
            *altered.pointer_mut(pointer).unwrap() = change;
            assert!(compare(live, &altered).is_err(), "{pointer}");
        }
        let mut extra = observed.clone();
        extra["entries"]["other"] = json!({"type":"file","mode":"0600","bytes":0});
        assert!(compare(live, &extra).is_err());
        Ok(())
    }
}
