use super::*;
use serde::Serialize;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::process::Stdio;
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct Status<'a> {
    schema: u8,
    operation: &'a str,
    version: Option<String>,
    target: Option<String>,
    active: bool,
    generation: u8,
    prefix: &'a Path,
    bin_dir: &'a Path,
}
#[derive(Serialize)]
struct Seed<'a> {
    format: u8,
    install_id: &'a str,
    prefix: &'a Path,
    bin_dir: &'a Path,
    generation: u8,
    active: Option<String>,
    launchers: Vec<String>,
}
fn line(value: impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn status(prefix: &Path, bin_dir: &Path) -> Result<Vec<u8>> {
    line(Status {
        schema: 1,
        operation: "status",
        version: None,
        target: None,
        active: false,
        generation: 0,
        prefix,
        bin_dir,
    })
}
fn seed(prefix: &Path, bin_dir: &Path) -> Result<Vec<u8>> {
    line(Seed {
        format: 1,
        install_id: "0123456789abcdef0123456789abcdef",
        prefix,
        bin_dir,
        generation: 0,
        active: None,
        launchers: vec![],
    })
}
fn snapshot(root: &Path, path: &Path, files: &mut Map) -> Result<()> {
    for child in fs::read_dir(path)? {
        let child = child?.path();
        let meta = fs::symlink_metadata(&child)?;
        ensure!(
            meta.is_dir() || meta.is_file(),
            "unexpected fixture file type"
        );
        let id = child
            .strip_prefix(root)?
            .to_str()
            .context("invalid fixture path")?
            .to_owned();
        let hash = if meta.is_file() {
            Some(digest(&read(&child)?))
        } else {
            None
        };
        files.insert(
            id,
            json!({"mode":format!("0o{:o}",meta.mode()),"sha256":hash}),
        );
        if meta.is_dir() {
            snapshot(root, &child, files)?;
        }
    }
    Ok(())
}
pub fn observe(
    tmp: &Path,
    report: &Path,
    revision: &str,
    policy: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Value> {
    let base = tmp.join("installed-layout");
    let prefix = base.join("prefix");
    let bin_dir = base.join("bin");
    let state = base.join("state");
    for dir in [&prefix, &bin_dir, &state] {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    use std::io::Write;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(prefix.join(".marmish-native-install.json"))?
        .write_all(&seed(&prefix, &bin_dir)?)?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(prefix.join(".lock"))?;
    let mut before = Map::new();
    snapshot(&base, &base, &mut before)?;
    let (program, args) = policy.split_first().context("missing native policy")?;
    let mut child = Command::new(program)
        .env_clear()
        .envs(environment)
        .args(args)
        .arg(tmp.join("target/release/marmish"))
        .args(["install-status", "--prefix"])
        .arg(&prefix)
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--state-dir")
        .arg(&state)
        .current_dir(tmp.join("source"))
        .stdin(Stdio::null())
        .stdout(fs::File::create(report.join("install-status.stdout"))?)
        .stderr(fs::File::create(report.join("install-status.stderr"))?)
        .spawn()?;
    let start = Instant::now();
    let exit = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill()?;
            child.wait()?;
            bail!("install-status timed out");
        }
        std::thread::sleep(Duration::from_millis(15));
    };
    let mut after = Map::new();
    snapshot(&base, &base, &mut after)?;
    let mut builds = vec![];
    for dir in fs::read_dir(tmp.join("target/release/build"))? {
        let path = dir?.path();
        if path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("marmish-native-")
            && path.join("output").exists()
        {
            builds.push(path.join("output"));
        }
    }
    ensure!(builds.len() == 1, "release build provenance unavailable");
    let raw = read(&builds[0])?;
    let metadata = std::str::from_utf8(&raw)?;
    ensure!(
        metadata
            .lines()
            .any(|l| l == format!("cargo:rustc-env=MARMISH_BUILD_COMMIT={revision}"))
            && metadata
                .lines()
                .any(|l| l == format!("cargo:rustc-env=MARMISH_BUILD_TARGET={TARGET}")),
        "release binary compiled for unexpected source"
    );
    fs::write(report.join("build-metadata.txt"), &raw)?;
    Ok(json!({
        "sourceBuild":{"commit":revision,"target":TARGET,"metadataSha256":digest(&raw)},
        "installedStatus":{
            "scenario":"inactive pre-seeded layout","exitCode":exit.code(),
            "stdoutMatchesExpected":read(&report.join("install-status.stdout"))?==status(&prefix,&bin_dir)?,
            "stderrEmpty":read(&report.join("install-status.stderr"))?.is_empty(),
            "fixtureUnchanged":before==after,"fixtureBefore":before,"fixtureAfter":after
        }
    }))
}
pub fn check(dir: &Path, probe: &Value) -> Result<()> {
    let raw = read(&dir.join("install-status.stdout"))?;
    let observed: Value = serde_json::from_slice(&raw)?;
    let prefix = Path::new(text(&observed["prefix"])?);
    let bin_dir = Path::new(text(&observed["bin_dir"])?);
    ensure!(
        prefix.ends_with("prefix")
            && bin_dir.ends_with("bin")
            && prefix.parent() == bin_dir.parent()
            && prefix
                .parent()
                .is_some_and(|p| p.ends_with("installed-layout")),
        "wrong status fixture paths"
    );
    ensure!(
        raw == status(prefix, bin_dir)? && read(&dir.join("install-status.stderr"))?.is_empty(),
        "wrong installed status output"
    );
    let fixture = object(&probe["fixtureBefore"])?;
    ensure!(
        fixture.keys().cloned().collect::<Set>()
            == set(&[
                "prefix",
                "bin",
                "state",
                "prefix/.lock",
                "prefix/.marmish-native-install.json"
            ]),
        "incomplete layout snapshot"
    );
    let hashes = BTreeMap::from([
        ("prefix/.lock", digest(b"")),
        (
            "prefix/.marmish-native-install.json",
            digest(&seed(prefix, bin_dir)?),
        ),
    ]);
    for (path, record) in fixture {
        let (mode, hash) = if record.is_object() {
            (
                u32::from_str_radix(text(&record["mode"])?.trim_start_matches("0o"), 8)? & 0o7777,
                &record["sha256"],
            )
        } else {
            (
                u32::try_from(record[0].as_u64().context("invalid fixture mode")?)?,
                &record[1],
            )
        };
        let pinned = hashes.get(path.as_str());
        ensure!(
            mode == if pinned.is_some() { 0o600 } else { 0o700 } && *hash == json!(pinned),
            "fixture state differs from seeded inactive layout"
        );
    }
    Ok(())
}
