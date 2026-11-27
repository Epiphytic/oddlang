use provider_validation::{Result, cli, pin, read, sha, subprocess, write};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

fn execute(cmd: &mut Command, seconds: u64) -> Result<Output> {
    subprocess::run_with_timeout(cmd, None, Duration::from_secs(seconds))
}
fn success(cmd: &mut Command) -> Result<Vec<u8>> {
    let result = execute(cmd, 120)?;
    if !result.status.success() {
        return Err(format!(
            "{cmd:?}: {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result.stdout)
}
fn decline(cmd: &mut Command, reason: &str) -> Result {
    let out = execute(cmd, 5)?;
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    assert_eq!(out.stderr, format!("{reason}\n").as_bytes());
    Ok(())
}
fn compile(cli: &Path, root: &Path, case: &Value) -> Result<PathBuf> {
    let lock = root.join("lock-source");
    fs::create_dir_all(lock.join("src"))?;
    fs::write(lock.join("src/lib.rs"), "")?;
    let mut toml = String::from(
        "[package]\nname=\"field-survey\"\nversion=\"1.0.0\"\nedition=\"2021\"\n[dependencies]\n",
    );
    for (name, dep) in case["dependencies"].as_object().unwrap() {
        toml.push_str(&format!(
            "{name} = {{version=\"={}\",features={}}}\n",
            dep["version"].as_str().unwrap(),
            dep["features"]
        ));
    }
    toml.push_str("[workspace]\n");
    fs::write(lock.join("Cargo.toml"), toml)?;
    success(
        Command::new("cargo")
            .args(["+1.97.1", "generate-lockfile", "--offline"])
            .current_dir(&lock),
    )?;
    for (name, key) in [
        ("cli.json", "program"),
        ("cli-rust.json", "command"),
        ("profile.json", "graph"),
        ("profile-rust.json", "overlay"),
    ] {
        write(&root.join(name), &case[key])?;
    }
    let manifest = json!({"format":"oddlang-native-package-v1","name":"field-survey","version":"1.0.0","edition":"2021",
        "dependencies":case["dependencies"],"cargoLock":pin(root,"lock-source/Cargo.lock")?,
        "components":[{"kind":"cli-surface","module":"cli","graph":pin(root,"cli.json")?,"overlay":pin(root,"cli-rust.json")?,"reexports":["run_cli"]},
        {"kind":"file-profile","module":"keys","graph":pin(root,"profile.json")?,"overlay":pin(root,"profile-rust.json")?,"reexports":["prepare_credentials"],"rootReexports":[]}],
        "binaries":[{"name":"field-survey","entry":["run_cli"]}]});
    write(&root.join("package.json"), &manifest)?;
    success(Command::new(cli).current_dir(root).args([
        "build",
        "file-profile",
        "rust",
        "profile.json",
        "profile-rust.json",
        "projection",
    ]))?;
    assert_eq!(
        read(&root.join("projection/projection.lock"))?["obligationsSha256"],
        sha(&root.join("projection/profile-obligations.json"))?
    );
    success(Command::new(cli).current_dir(root).args([
        "build",
        "native-package",
        "rust",
        "package.json",
        "package",
    ]))?;
    let package = root.join("package");
    success(Command::new("cargo").current_dir(&package).args([
        "+1.97.1",
        "build",
        "--locked",
        "--offline",
        "--release",
        "--target-dir",
        "target",
    ]))?;
    Ok(package.join("target/release/field-survey"))
}
fn prepare(binary: &Path, state: &Path, name: Option<&str>) -> Command {
    let mut cmd = Command::new(binary);
    cmd.arg("--station").arg(state).args(["--medium", "local"]);
    if let Some(name) = name {
        cmd.args(["--name", name]);
    }
    cmd.arg("prepare");
    cmd
}
fn chmod(path: &Path, mode: u32) -> Result {
    Ok(fs::set_permissions(path, fs::Permissions::from_mode(mode))?)
}
fn directory(path: &Path, mode: u32) -> Result {
    fs::DirBuilder::new().mode(mode).create(path)?;
    chmod(path, mode)
}
fn metadata(path: &Path) -> Result<(u64, String)> {
    Ok((path.metadata()?.ino(), sha(path)?))
}
fn provision(state: &Path, name: &str, document: &Value) -> Result<PathBuf> {
    let profile = state.join(name);
    directory(&profile, 0o700)?;
    directory(&profile.join("credentials"), 0o700)?;
    let manifest = profile.join("record.json");
    let mut data = document.clone();
    data["label"] = name.into();
    data["attestation"] = Value::Null;
    write(&manifest, &data)?;
    chmod(&manifest, 0o600)?;
    Ok(profile)
}
fn scalar(path: &Path, value: u8) -> Result {
    let mut bytes = [0; 32];
    bytes[31] = value;
    fs::write(path, bytes)?;
    chmod(path, 0o600)
}
pub(super) fn verify() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().canonicalize()?;
    let case = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../file-profile/survey.json"))?;
    let binary = compile(&cli, &root, &case)?;
    let state = root.join("state");
    let default = || prepare(&binary, &state, None);
    let first = success(&mut default())?;
    let profile = state.join("daily");
    let document = read(&profile.join("record.json"))?;
    assert_eq!(document["v"], 9);
    assert_eq!(document["stage"], "proposed");
    assert_eq!(document["provider"], "disk");
    assert_eq!(
        serde_json::from_slice::<Value>(&first)?["identity"],
        document["attestation"]
    );
    let mut credentials = Vec::new();
    for entry in fs::read_dir(profile.join("credentials"))? {
        let file = entry?.path();
        assert_eq!(fs::read(&file)?.len(), 32);
        assert_eq!(file.metadata()?.mode() & 0o7777, 0o600);
        credentials.push((file.clone(), metadata(&file)?));
    }
    assert_eq!(credentials.len(), 2);
    assert_eq!(success(&mut default())?, first);
    for (file, before) in &credentials {
        assert_eq!(&metadata(file)?, before);
    }
    let (occupied, (_, digest)) = &credentials[0];
    chmod(occupied, 0o644)?;
    decline(&mut default(), "unsafe file")?;
    assert_eq!(&sha(occupied)?, digest);
    assert_eq!(profile.metadata()?.mode() & 0o777, 0o700);
    chmod(occupied, 0o600)?;
    let lease = profile.join("lease");
    {
        let held = fs::File::open(&lease)?;
        held.try_lock()?;
        decline(&mut default(), "in use")?;
    }
    let link = profile.join("credentials/copy");
    fs::hard_link(occupied, &link)?;
    decline(&mut default(), "unsafe file")?;
    fs::remove_file(link)?;
    let old = profile.join("previous-lease");
    fs::rename(&lease, &old)?;
    symlink(&old, &lease)?;
    decline(&mut default(), "io")?;
    assert!(lease.is_symlink() && fs::read(&old)?.is_empty());
    fs::remove_file(&lease)?;
    fs::rename(old, lease)?;
    let resume = provision(&state, "resume", &document)?;
    let signing = resume
        .join("credentials")
        .join(format!("signature-{}", document["sign"].as_str().unwrap()));
    scalar(&signing, 1)?;
    let inode = signing.metadata()?.ino();
    let resume_cmd = || prepare(&binary, &state, Some("resume"));
    let observed: Value = serde_json::from_slice(&success(&mut resume_cmd())?)?;
    assert_eq!(
        observed["identity"],
        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
    );
    assert_eq!(read(&resume.join("record.json"))?["stage"], "proposed");
    assert!(
        resume
            .join("credentials")
            .join(format!("vault-{}", document["locker"].as_str().unwrap()))
            .is_file()
    );
    assert_eq!(signing.metadata()?.ino(), inode);
    fs::remove_file(&signing)?;
    decline(&mut resume_cmd(), "missing secret")?;
    scalar(&signing, 2)?;
    decline(&mut resume_cmd(), "identity mismatch")?;
    let initial = provision(&state, "initial", &document)?;
    success(&mut prepare(&binary, &state, Some("initial")))?;
    assert_eq!(fs::read_dir(initial.join("credentials"))?.count(), 2);
    assert_eq!(read(&initial.join("record.json"))?["stage"], "proposed");
    let unsafe_dir = root.join("unsafe");
    directory(&unsafe_dir, 0o755)?;
    decline(&mut prepare(&binary, &unsafe_dir, None), "unsafe directory")?;
    assert_eq!(fs::read_dir(unsafe_dir)?.count(), 0);
    let mut invalid = case["graph"].clone();
    invalid["layout"]["owner"] = "../lease".into();
    write(&root.join("invalid.json"), &invalid)?;
    let bad = execute(
        Command::new(&cli).current_dir(&root).args([
            "build",
            "file-profile",
            "rust",
            "invalid.json",
            "profile-rust.json",
            "invalid",
        ]),
        5,
    )?;
    assert!(
        !bad.status.success()
            && String::from_utf8_lossy(&bad.stderr).contains("invalid private file profile layout")
    );
    assert!(!root.join("invalid").exists());
    println!(
        "File-profile credential bytes, identity, file modes, inode preservation, locks and rejection controls passed"
    );
    Ok(())
}
