//! Online preparation for the offline component suites. This crate has no dependencies.
#[path = "../../wasm_pins.rs"]
mod manifest;
use manifest::Pins;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn run(cmd: &mut Command) -> Result {
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("{cmd:?} exited {status}").into());
    }
    Ok(())
}
fn stdout(cmd: &mut Command) -> Result<String> {
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(format!(
            "{cmd:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
fn verify(file: &Path, expected: &str) -> Result {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("shasum");
        c.args(["-a", "256"]);
        c
    } else {
        Command::new("sha256sum")
    };
    let result = stdout(cmd.arg(file))?;
    if result.split_whitespace().next() != Some(expected) {
        return Err(format!(
            "SHA-256 mismatch for {}",
            file.file_name().unwrap_or_default().to_string_lossy()
        )
        .into());
    }
    Ok(())
}
fn download(url: &str, path: &Path, expected: &str) -> Result {
    run(Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--output",
        ])
        .arg(path)
        .arg(url))?;
    verify(path, expected)
}
struct Stage(PathBuf);
impl Stage {
    fn create(cache: &Path) -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = cache.join(format!("setup-{}-{nonce}", std::process::id()));
        fs::create_dir(&dir)?;
        Ok(Self(dir))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn check_rust(p: &Pins) -> Result {
    let toolchain = format!("+{}", p.rust_version);
    let version = stdout(
        Command::new("rustc")
            .arg(&toolchain)
            .arg("-vV")
            .env("RUSTUP_AUTO_INSTALL", "0"),
    )?;
    if !version
        .lines()
        .any(|l| l == format!("commit-hash: {}", p.rust_commit))
        || !version
            .lines()
            .any(|l| l == format!("release: {}", p.rust_version))
    {
        return Err("installed Rust compiler differs from the pinned release or commit".into());
    }
    let installed = stdout(Command::new("rustup").args([
        "target",
        "list",
        "--installed",
        "--toolchain",
        &p.rust_version,
    ]))?;
    for t in [&p.guest_target, &p.ledger_guest_target] {
        if !installed.lines().any(|s| s == t) {
            return Err(format!("Rust target missing after install: {t}").into());
        }
    }
    Ok(())
}
fn fetch(root: &Path, p: &Pins) -> Result {
    let toolchain = format!("+{}", p.rust_version);
    for manifest in [
        "examples/wasm/Cargo.toml",
        "Cargo.toml",
        "verification/provider-validation/Cargo.toml",
        "examples/wasm/ledger-guests/Cargo.toml",
    ] {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(root).arg(&toolchain).args([
            "fetch",
            "--locked",
            "--manifest-path",
            manifest,
        ]);
        if manifest == "examples/wasm/ledger-guests/Cargo.toml" {
            cmd.arg("--target").arg(&p.ledger_guest_target);
        }
        run(&mut cmd)?;
    }
    Ok(())
}
/// Preserve additional files owned by the verification commands in target/wasm-js.
fn publish_js(source: &Path, dest: &Path, stage: &Path) -> Result {
    fs::create_dir_all(dest)?;
    let mut backups = vec![];
    let mut written = vec![];
    let mut publish = || -> io::Result<()> {
        for name in ["package.json", "package-lock.json", "node_modules"] {
            let target = dest.join(name);
            match fs::symlink_metadata(&target) {
                Ok(_) => {
                    let backup = stage.join(format!("previous-{name}"));
                    fs::rename(&target, &backup)?;
                    backups.push((backup, target.clone()));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            fs::rename(source.join(name), &target)?;
            written.push((target, source.join(name)));
        }
        Ok(())
    };
    if let Err(err) = publish() {
        for (installed, staged) in written.into_iter().rev() {
            fs::rename(installed, staged)?;
        }
        for (backup, original) in backups.into_iter().rev() {
            fs::rename(backup, original)?;
        }
        return Err(err.into());
    }
    Ok(())
}
fn setup(root: &Path) -> Result {
    let p = Pins::load(root)?;
    let (archive, expected) = p.archive(env::consts::ARCH, env::consts::OS)?;
    let cache = root.join("target/wasm-tools");
    fs::create_dir_all(&cache)?;
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cache.join("setup.lock"))?;
    lock.lock()?;
    let stage = Stage::create(&cache)?;
    let filename = format!("{archive}.tar.gz");
    download(
        &format!(
            "https://static.rust-lang.org/dist/channel-rust-{}.toml",
            p.rust_version
        ),
        &stage.0.join("rust.toml"),
        &p.rust_manifest_sha256,
    )?;
    download(
        &format!(
            "https://github.com/bytecodealliance/wasm-tools/releases/download/v{}/{filename}",
            p.wasm_tools_version
        ),
        &stage.0.join(&filename),
        expected,
    )?;
    run(Command::new("rustup").args([
        "toolchain",
        "install",
        &p.rust_version,
        "--profile",
        "minimal",
        "--target",
        &p.guest_target,
        "--target",
        &p.ledger_guest_target,
        "--no-self-update",
    ]))?;
    check_rust(&p)?;
    fetch(root, &p)?;
    let js = stage.0.join("javascript");
    fs::create_dir(&js)?;
    for name in ["package.json", "package-lock.json"] {
        fs::copy(
            root.join("examples/wasm/javascript").join(name),
            js.join(name),
        )?;
    }
    run(Command::new("npm")
        .current_dir(&js)
        .args(["ci", "--ignore-scripts", "--no-audit"]))?;
    run(Command::new(js.join("node_modules/.bin/playwright"))
        .current_dir(&js)
        .args(["install", "chromium"]))?;
    publish_js(&js, &root.join("target/wasm-js"), &stage.0)?;
    fs::rename(stage.0.join(&filename), cache.join(filename))?;
    println!(
        "Prepared Rust {} ({}, {}), {} and Chromium for offline {} builds.",
        p.rust_version, p.guest_target, p.ledger_guest_target, archive, p.guest_profile
    );
    Ok(())
}
fn main() -> Result {
    let mut args = env::args_os().skip(1);
    let root = match args.next() {
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        Some(arg) if arg == "--help" || arg == "-h" => {
            println!("wasm-setup [--root CHECKOUT]");
            return Ok(());
        }
        Some(arg) if arg == "--root" => {
            PathBuf::from(args.next().ok_or("missing checkout after --root")?)
        }
        _ => return Err("usage: wasm-setup [--root CHECKOUT]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected extra arguments".into());
    }
    setup(&root.canonicalize()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuse_a_download_with_the_wrong_digest() -> Result {
        let stage = Stage::create(&env::temp_dir())?;
        let file = stage.0.join("download");
        fs::write(&file, b"abc")?;
        let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        verify(&file, sha)?;
        fs::write(&file, b"changed")?;
        assert!(verify(&file, sha)
            .unwrap_err()
            .to_string()
            .contains("SHA-256 mismatch"));
        Ok(())
    }

    #[test]
    fn failed_refresh_preserves_old_javascript_install_and_consumer_files() -> Result {
        let stage = Stage::create(&env::temp_dir())?;
        let source = stage.0.join("new");
        let dest = stage.0.join("installed");
        fs::create_dir(&source)?;
        fs::create_dir(&dest)?;
        for name in ["package.json", "package-lock.json"] {
            fs::write(source.join(name), b"updated")?;
            fs::write(dest.join(name), b"previous")?;
        }
        fs::create_dir(dest.join("node_modules"))?;
        fs::write(dest.join("node_modules/tool"), b"previous tool")?;
        fs::write(dest.join("run.mjs"), b"consumer")?;
        assert!(publish_js(&source, &dest, &stage.0).is_err());
        for name in ["package.json", "package-lock.json"] {
            assert_eq!(fs::read(dest.join(name))?, b"previous");
        }
        assert_eq!(fs::read(dest.join("node_modules/tool"))?, b"previous tool");
        fs::create_dir(source.join("node_modules"))?;
        fs::write(source.join("node_modules/tool"), b"new tool")?;
        publish_js(&source, &dest, &stage.0)?;
        assert_eq!(fs::read(dest.join("node_modules/tool"))?, b"new tool");
        assert_eq!(fs::read(dest.join("run.mjs"))?, b"consumer");
        Ok(())
    }
}
