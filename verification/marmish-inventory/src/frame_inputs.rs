use anyhow::{Context, Result, ensure};
use marmish_inventory::{Map, digest, file_hashes, read, read_json};
use serde_json::{Value, json};
use std::{fs, path::Path};
pub fn verify_inputs(imported: &Path, positive: &Path, mutant: &Path) -> Result<()> {
    let graph = digest(&read(&imported.join("domain.ttl"))?);
    let lock = read_json(&imported.join("import.lock"))?;
    ensure!(
        lock["graphSha256"] == graph && lock["profile"] == "framing-v1",
        "import lock disagrees with the framing graph"
    );
    for dir in [positive, mutant] {
        let manifest = read_json(&dir.join("projection.json"))?;
        ensure!(
            manifest["version"] == 1 && manifest["files"] == file_hashes(dir, &["frame.rs"])?,
            "projection does not match its pinned source bytes"
        );
        ensure!(
            (dir == positive) == (manifest["graphSha256"] == graph),
            "expected distinct positive and mutant graphs"
        );
    }
    ensure!(
        read(&positive.join("frame.rs"))? != read(&mutant.join("frame.rs"))?,
        "mutant and production code are identical"
    );
    ensure!(
        read(&positive.join("world.wit"))? == read(&mutant.join("world.wit"))?,
        "mutant changed the public component ABI"
    );
    Ok(())
}
fn harness_files(dir: &Path, root: &Path, out: &mut Map) -> Result<()> {
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if path.is_dir() {
            harness_files(&path, root, out)?;
        } else {
            out.insert(
                path.strip_prefix(root)?
                    .to_str()
                    .context("invalid path")?
                    .into(),
                json!(digest(&read(&path)?)),
            );
        }
    }
    Ok(())
}

pub fn harness(root: &Path) -> Result<Value> {
    let mut files = Map::new();
    harness_files(&root.join("examples/wasm/framing"), root, &mut files)?;
    for name in [
        "examples/wasm/Cargo.lock",
        "examples/wasm/host/Cargo.toml",
        "examples/wasm/toolchain.lock",
        "scripts/marmish/frame.sh",
        "scripts/marmish/observe_core.rs",
        "scripts/marmish/test_base64_tails.rs",
        "verification/marmish-inventory/Cargo.toml",
        "verification/marmish-inventory/Cargo.lock",
        "verification/marmish-inventory/src/lib.rs",
        "verification/marmish-inventory/src/framing_contract.rs",
        "verification/marmish-inventory/src/frame_inputs.rs",
        "verification/marmish-inventory/src/bin/framing-observer.rs",
        "verification/marmish-inventory/src/bin/frame-matrix.rs",
    ] {
        files.insert(name.into(), json!(digest(&read(&root.join(name))?)));
    }
    Ok(Value::Object(files))
}
