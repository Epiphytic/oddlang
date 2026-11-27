use super::*;
use flate2::read::GzDecoder;
use std::io::Read;
pub const ARCHIVE_HASH: &str = "4c944d14b9260b2300a164cedda7f8721cb0e487efd1ef7e515c5a900f8fea32";
pub const TREE: &str = "c78b36ae6720565170bc6dce578531f58ab671c7";
fn archive() -> Result<tar::Archive<GzDecoder<std::io::Cursor<Vec<u8>>>>> {
    let bytes = read(&root().join("examples/marmish/evidence/original.tar.gz"))?;
    ensure!(
        digest(&bytes) == ARCHIVE_HASH,
        "published original archive changed"
    );
    Ok(tar::Archive::new(GzDecoder::new(std::io::Cursor::new(
        bytes,
    ))))
}
fn check(dir: &Path) -> Result<()> {
    let result = Command::new("cargo")
        .args([
            "+1.97.1",
            "run",
            "--quiet",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(root().join("verification/marmish-inventory/Cargo.toml"))
        .args(["--bin", "marmish-receipt", "--", "check"])
        .arg(dir)
        .output()?;
    ensure!(
        result.status.success(),
        "invalid original receipt: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
pub fn authenticate(dir: &Path) -> Result<()> {
    let receipt = read(&dir.join("receipt.json"))?;
    for entry in archive()?.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new("receipt.json") {
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes)?;
            ensure!(receipt == bytes, "recovered original receipt changed");
            return check(dir);
        }
    }
    bail!("published original receipt missing")
}
pub fn recover(dir: &Path) -> Result<Value> {
    let mut archive = archive()?;
    fs::create_dir_all(dir)?;
    ensure!(
        fs::read_dir(dir)?.next().is_none(),
        "original archive needs empty destination"
    );
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            entry.header().entry_type().is_file()
                && path
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "invalid member in original archive"
        );
        let dest = dir.join(path);
        fs::create_dir_all(dest.parent().unwrap())?;
        let mut file = fs::File::create_new(dest)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    check(dir)?;
    ensure!(
        read_json(&dir.join("native/baseline.json"))?["tree"] == TREE,
        "original Git tree changed"
    );
    ensure!(
        read_json(&dir.join("inventory/inventory-summary.json"))?
            == read_json(&root().join("examples/marmish/inventory-summary.json"))?,
        "original inventory differs from checked summary"
    );
    let summary = regex::Regex::new(
        r"(?m)^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; 0 filtered out;",
    )?;
    let failed = regex::Regex::new(r"(?m)^test (\S+) \.\.\. FAILED$")?;
    let mut observed = json!({});
    for (mode, expected) in [("workspace", [906, 9, 5]), ("installed", [907, 8, 3])] {
        let raw = fs::read_to_string(dir.join(format!("native/logs/{mode}.stdout")))?;
        let mut counts = [0usize; 3];
        for m in summary.captures_iter(&raw) {
            for (i, count) in counts.iter_mut().enumerate() {
                *count += m[i + 1].parse::<usize>()?;
            }
        }
        ensure!(
            counts == expected,
            "published {mode} summary disagrees with test transcripts"
        );
        let names: Set = failed
            .captures_iter(&raw)
            .map(|m| m[1].to_owned())
            .collect();
        observed[mode] = json!({"counts":{"passed":counts[0],"failed":counts[1],"ignored":counts[2]},"failedTests":names});
    }
    Ok(observed)
}
