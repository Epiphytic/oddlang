use super::{Result, require};
use provider_validation::{pin, read, subprocess::run, write};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
pub fn private_dir(path: &Path) -> Result {
    fs::DirBuilder::new().mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub struct Station {
    pub profile: PathBuf,
    pub signing: PathBuf,
    pub backup: PathBuf,
}
pub fn scalar(n: u8) -> [u8; 32] {
    let mut b = [0; 32];
    b[31] = n;
    b
}
impl Station {
    pub fn create(root: &Path) -> Result<Self> {
        for d in ["state", "runtime", "home"] {
            private_dir(&root.join(d))?;
        }
        let profile = root.join("state/pond");
        private_dir(&profile)?;
        let credentials = profile.join("credentials");
        private_dir(&credentials)?;
        let manifest = profile.join("station.json");
        write(
            &manifest,
            &json!({
                "header":3,"token":"11".repeat(8),"label":"pond","change":"22".repeat(8),"phase":"recorded","store":"disk",
                "signing":"33".repeat(12),"backup":"44".repeat(12),"device":"55".repeat(8),
                "pub":"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
            }),
        )?;
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600))?;
        let signing = credentials.join(format!("attest-{}", "33".repeat(12)));
        let backup = credentials.join(format!("backup-{}", "44".repeat(12)));
        for (p, n) in [(&signing, 1), (&backup, 2)] {
            fs::write(p, scalar(n))?;
            fs::set_permissions(p, fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self {
            profile,
            signing,
            backup,
        })
    }
}
pub fn project(root: &Path, source: &Path, cli: &Path) -> Result<(PathBuf, Value)> {
    fs::copy(source.join("Cargo.lock"), root.join("Cargo.lock"))?;
    fs::copy(
        source.join("../local-status-owner/graph.json"),
        root.join("owner.json"),
    )?;
    // These document bytes are the original station inputs. Their digests also bind package.lock.
    for name in [
        "cli.json",
        "cli-rust.json",
        "owner-rust.json",
        "profile.json",
        "profile-rust.json",
        "package.json",
    ] {
        fs::copy(source.join("station").join(name), root.join(name))?;
    }
    let package = read(&root.join("package.json"))?;
    let generated = root.join("generated");
    let build = run(
        Command::new(cli).current_dir(root).args([
            "build",
            "native-package",
            "rust",
            "package.json",
            "generated",
        ]),
        None,
    )?;
    require(
        build.status.success(),
        &format!(
            "station projection failed: {}",
            String::from_utf8_lossy(&build.stderr)
        ),
    )?;
    let target = root.join("build");
    let native = run(
        Command::new("cargo")
            .current_dir(&generated)
            .args([
                "+1.97.1",
                "build",
                "--locked",
                "--offline",
                "--release",
                "--target-dir",
            ])
            .arg(&target),
        None,
    )?;
    require(
        native.status.success(),
        &format!(
            "generated station build failed: {}",
            String::from_utf8_lossy(&native.stderr)
        ),
    )?;
    Ok((target.join("release/field-station"), package))
}
pub fn reject_invalid_links(cli: &Path, root: &Path, package: &Value) -> Result {
    let mut graph = read(&root.join("profile.json"))?;
    graph["arguments"]["backend_choices"]["remote"] = json!("other-provider");
    write(&root.join("bad-choice.json"), &graph)?;
    let mut overlay = read(&root.join("profile-rust.json"))?;
    overlay["status_owner"] = json!(["other", "BoundOwner"]);
    write(&root.join("bad-owner.json"), &overlay)?;
    for (field, file, output, diagnostic) in [
        (
            "graph",
            "bad-choice.json",
            "rejected-choice",
            "invalid foreground owner arguments",
        ),
        (
            "overlay",
            "bad-owner.json",
            "rejected",
            "requires a linked local status owner",
        ),
    ] {
        let mut bad = package.clone();
        bad["components"][2][field] = pin(root, file)?;
        write(&root.join("bad-package.json"), &bad)?;
        let o = provider_validation::subprocess::run_with_timeout(
            Command::new(cli).current_dir(root).args([
                "build",
                "native-package",
                "rust",
                "bad-package.json",
                output,
            ]),
            None,
            std::time::Duration::from_secs(10),
        )?;
        require(
            !o.status.success()
                && String::from_utf8_lossy(&o.stderr).contains(diagnostic)
                && !root.join(output).exists(),
            &format!("unexpected linker outcome for {file}"),
        )?;
    }
    Ok(())
}
