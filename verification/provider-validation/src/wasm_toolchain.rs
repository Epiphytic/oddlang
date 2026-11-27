//! Shared pinned build inputs for the offline WebAssembly qualification tools.
use crate::{Result, sha};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
pub struct Pins {
    pub rust_version: String,
    rust_commit: String,
    pub guest_target: String,
    pub ledger_guest_target: String,
    pub guest_profile: String,
    wasm_tools_version: String,
    #[serde(flatten)]
    other: BTreeMap<String, String>,
}

pub fn run(cmd: &mut Command) -> Result {
    cmd.env("CARGO_NET_OFFLINE", "true")
        .env("RUSTUP_AUTO_INSTALL", "0");
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("{cmd:?}: {status}").into());
    }
    Ok(())
}

pub fn copy_tree(src: &Path, dest: &Path) -> Result {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub fn prepare(root: &Path, cache: &Path, stage: &Path) -> Result<(Pins, PathBuf)> {
    let pins: Pins = toml::from_str(&fs::read_to_string(
        root.join("examples/wasm/toolchain.toml"),
    )?)?;
    let platform = format!("{}-{}", env::consts::ARCH, env::consts::OS);
    let expected = pins
        .other
        .get(&platform.replace('-', "_"))
        .ok_or("unsupported wasm-tools host")?;
    let archive_name = format!("wasm-tools-{}-{platform}", pins.wasm_tools_version);
    let archive = cache.join(format!("{archive_name}.tar.gz"));
    let checked_archive = stage.join("wasm-tools.tar.gz");
    fs::copy(&archive, &checked_archive)?;
    if sha(&checked_archive)? != *expected {
        return Err("wasm-tools archive digest mismatch".into());
    }
    let toolchain = format!("+{}", pins.rust_version);
    let version = Command::new("rustc")
        .arg(&toolchain)
        .arg("-Vv")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()?;
    if !version.status.success()
        || !String::from_utf8(version.stdout)?
            .lines()
            .any(|s| s == format!("commit-hash: {}", pins.rust_commit))
    {
        return Err("guest rustc commit mismatch".into());
    }
    let member = format!("{archive_name}/wasm-tools");
    run(Command::new("tar")
        .arg("-xzf")
        .arg(checked_archive)
        .arg("-C")
        .arg(stage)
        .arg(&member))?;
    fs::remove_file(stage.join("wasm-tools.tar.gz"))?;
    Ok((pins, stage.join(member)))
}

pub fn compiler(executable: Option<&Path>, version: &str) -> Result<Command> {
    Ok(if let Some(path) = executable {
        Command::new(path.canonicalize()?)
    } else {
        let mut cmd = Command::new("cargo");
        cmd.arg(format!("+{version}")).args([
            "run",
            "--quiet",
            "--locked",
            "--offline",
            "--bin",
            "oddlang",
            "--",
        ]);
        cmd
    })
}

/// Publish the pinned three-world fixture; the test world imports the ledger release.
pub fn publish_fixture(mut cli: Command, root: &Path, out: &Path, name: &str) -> Result<PathBuf> {
    let store = out.join("cache");
    let lock = if name == "ledger-test" {
        let key = format!("{:x}", Sha256::digest(b"urn:oddlang:ledger:wasm:package"));
        copy_tree(
            &out.join("ledger"),
            &store.join("wasm").join(key).join("1.0.0"),
        )?;
        "contracts/wasm/ledger-test/dependencies.lock.json"
    } else {
        "contracts/wasm/empty.lock.json"
    };
    let bundle = out.join(name);
    run(cli
        .current_dir(root)
        .args(["wasm", "publish"])
        .arg(format!("contracts/wasm/{name}"))
        .arg(lock)
        .arg(store)
        .arg(&bundle))?;
    Ok(bundle)
}
