use super::*;
use std::os::unix::fs::{DirBuilderExt, symlink};

pub(super) fn tool(command: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .with_context(|| format!("start {command}"))?;
    ensure!(
        output.status.success(),
        "{command} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub(super) fn private_dir(dir: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("create {}", dir.display()))
}
fn named_path(path: &Path) -> Result<String> {
    Ok(path.to_str().context("non-UTF8 tool path")?.into())
}
fn uv_buckets(cache: &Path, local: &Path) -> Result<()> {
    fs::create_dir(local)?;
    symlink(cache.join("archive-v0"), local.join("archive-v0"))?;
    for bucket in fs::read_dir(cache)? {
        let bucket = bucket?;
        let name = bucket.file_name();
        let Some((prefix, version)) = name.to_str().and_then(|n| n.split_once("-v")) else {
            continue;
        };
        if !matches!(prefix, "wheels" | "sdists" | "git" | "simple")
            || !version.starts_with(|c: char| c.is_ascii_digit())
        {
            continue;
        }
        let dest = local.join(&name);
        fs::create_dir(&dest)?;
        for item in fs::read_dir(bucket.path())? {
            let item = item?;
            let name = item.file_name();
            if !name.as_encoded_bytes().starts_with(b".") && name != "locks" {
                symlink(item.path(), dest.join(name))?;
            }
        }
    }
    Ok(())
}
pub(super) fn configure(tmp: &Path, report: &Path) -> Result<Environment> {
    let home = tmp.join("home");
    let temp = tmp.join("tmp");
    let cargo = tmp.join("cargo");
    for dir in [
        &home,
        &temp,
        &cargo,
        &report.join("logs"),
        &tmp.join("logs"),
    ] {
        private_dir(dir)?;
    }
    let cargo_bin = PathBuf::from(tool(
        "rustup",
        &["which", "--toolchain", "1.97.1", "cargo"],
    )?);
    let cargo_path = cargo_bin.parent().context("cargo path")?;
    let uv = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("uv"))
        .find(|p| p.is_file())
        .context("uv missing from PATH")?
        .canonicalize()?;
    let personal =
        PathBuf::from(std::env::var_os("HOME").context("missing HOME")?).canonicalize()?;
    let package = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| personal.join(".cargo"))
        .canonicalize()?;
    for name in ["registry", "git"] {
        symlink(package.join(name), cargo.join(name))?;
    }
    let python = PathBuf::from(tool(
        uv.to_str().unwrap(),
        &["python", "find", "--offline", "3.12"],
    )?)
    .canonicalize()?;
    let uv_cache = PathBuf::from(tool(uv.to_str().unwrap(), &["cache", "dir"])?).canonicalize()?;
    uv_buckets(&uv_cache, &tmp.join("uv-cache"))?;
    let mut policy = vec!["/usr/bin/sandbox-exec".to_owned()];
    for (name, path) in [
        ("WORK", tmp.to_owned()),
        ("PERSONAL", personal),
        ("CARGO_REGISTRY", package.join("registry")),
        ("CARGO_GIT", package.join("git")),
        (
            "TOOLCHAIN",
            cargo_path
                .parent()
                .context("missing toolchain root")?
                .to_owned(),
        ),
        (
            "PYTHON",
            python
                .ancestors()
                .nth(2)
                .context("missing Python root")?
                .to_owned(),
        ),
        ("UV", uv.clone()),
        ("UV_CACHE", uv_cache),
    ] {
        policy.extend(["-D".to_owned(), format!("{name}={}", path.display())]);
    }
    let profile = report.join("baseline-macos.sb");
    fs::copy(root().join("scripts/marmish/baseline-macos.sb"), &profile)?;
    policy.extend(["-f".into(), named_path(&profile)?]);
    let clang = PathBuf::from(tool("xcrun", &["--find", "clang"])?);
    let mut vars: BTreeMap<String, String> = [
        ("CARGO_NET_OFFLINE", "true"),
        ("UV_OFFLINE", "true"),
        ("UV_PYTHON_DOWNLOADS", "never"),
        ("UV_LINK_MODE", "copy"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("LC_ALL", "C"),
        ("TZ", "UTC"),
        ("RUST_TEST_THREADS", "1"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    vars.insert(
        "PATH".into(),
        format!(
            "{}:{}:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            cargo_path.display(),
            clang.parent().context("missing clang directory")?.display()
        ),
    );
    vars.insert("SDKROOT".into(), tool("xcrun", &["--show-sdk-path"])?);
    for (name, path) in [
        ("HOME", home),
        ("TMPDIR", temp),
        ("CARGO_HOME", cargo),
        ("CARGO_TARGET_DIR", tmp.join("target")),
        ("UV_CACHE_DIR", tmp.join("uv-cache")),
        ("UV_PYTHON", python.clone()),
    ] {
        vars.insert(name.into(), named_path(&path)?);
    }
    Ok(Environment {
        vars,
        policy,
        uv: named_path(&uv)?,
        python: named_path(&python)?,
    })
}
