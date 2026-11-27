//! Mutation controls against a frozen archive with real CLI logs.
use super::*;
use std::process::Command;
use tempfile::{TempDir, tempdir};

fn archive() -> Result<TempDir> {
    let temp = tempdir()?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/marmish/evidence/original.tar.gz");
    let ok = Command::new("tar")
        .arg("-xzf")
        .arg(path)
        .arg("-C")
        .arg(temp.path())
        .status()?;
    ensure!(ok.success(), "archive extraction failed");
    Ok(temp)
}
fn verify<F>(mutation: F, expected: Option<&str>) -> Result<()>
where
    F: FnOnce(&mut Value, &Path) -> Result<()>,
{
    let tmp = archive()?;
    let native_dir = tmp.path().join("native");
    let mut native = read(&native_dir.join("baseline.json"))?;
    mutation(&mut native, &native_dir)?;
    let result = verify_cli(
        &native,
        &native_dir,
        &serde_json::from_str::<Value>(CONTRACT)?["cli"],
    );
    if let Some(reason) = expected {
        assert!(result.unwrap_err().to_string().contains(reason));
    } else {
        result?;
    }
    Ok(())
}
fn row<'a>(native: &'a mut Value, name: &str) -> &'a mut Value {
    native["commands"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["name"] == name)
        .unwrap()
}
#[test]
fn six_product_probes() -> Result<()> {
    verify(|_, _| Ok(()), None)
}
#[test]
fn worker_exit() -> Result<()> {
    verify(
        |v, _| {
            let r = row(v, "marmish-worker-help");
            r["exitCode"] = json!(0);
            r["status"] = json!("passed");
            Ok(())
        },
        Some("unexpected CLI exit"),
    )
}
#[test]
fn changed_help_matching_hash() -> Result<()> {
    verify(
        |v, dir| {
            let r = row(v, "marmish-help");
            let bytes = b"new help text\n";
            fs::write(dir.join(text(&r["stdout"])?), bytes)?;
            r["stdoutSha256"] = json!(hash(bytes));
            Ok(())
        },
        Some("unexpected CLI bytes"),
    )
}
#[test]
fn missing_command() -> Result<()> {
    verify(
        |v, _| {
            v["commands"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r["name"] != "marmish-version");
            Ok(())
        },
        Some("missing or incorrect CLI command"),
    )
}
#[test]
fn failed_release() -> Result<()> {
    verify(
        |v, _| {
            let r = row(v, "release");
            r["exitCode"] = json!(1);
            r["status"] = json!("failed");
            Ok(())
        },
        Some("release build failed"),
    )
}
#[test]
fn edited_observation() -> Result<()> {
    let tmp = archive()?;
    let root = tmp.path();
    let stored = root.join("receipt.json");
    let original = fs::read(&stored)?;
    fs::remove_file(&stored)?;
    check(root, true)?;
    assert_eq!(fs::read(&stored)?, original);
    check(root, false)?;
    assert!(check(root, true).is_err());
    let path = root.join("observations/results.json");
    let mut v = read(&path)?;
    v["changed"] = json!(true);
    fs::write(path, serde_json::to_vec(&v)?)?;
    assert!(
        check(root, false)
            .unwrap_err()
            .to_string()
            .contains("changed after collection")
    );
    Ok(())
}
#[test]
fn wrong_revision() -> Result<()> {
    let tmp = archive()?;
    fs::write(
        tmp.path().join("native/baseline.json"),
        br#"{"revision":"wrong","commands":[1]}"#,
    )?;
    assert!(
        check(tmp.path(), false)
            .unwrap_err()
            .to_string()
            .contains("missing pinned native observations")
    );
    Ok(())
}
#[cfg(unix)]
#[test]
fn symlink_refused() -> Result<()> {
    let tmp = archive()?;
    std::os::unix::fs::symlink("baseline.json", tmp.path().join("native/linked.log"))?;
    assert!(
        check(tmp.path(), false)
            .unwrap_err()
            .to_string()
            .contains("unexpected symlink")
    );
    Ok(())
}
