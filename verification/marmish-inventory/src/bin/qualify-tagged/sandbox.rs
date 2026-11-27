use crate::native::{checked, command, run, sum};
use anyhow::{Context, Result, ensure};
use marmish_inventory::digest;
use serde_json::{Value as V, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};
pub type Denied = BTreeMap<String, PathBuf>;
pub fn environment<'a>(cmd: &'a mut Command, stage: &Path, sdk: Option<&Path>) -> &'a mut Command {
    cmd.current_dir(stage)
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "C"),
            ("LC_ALL", "C"),
            ("TZ", "UTC"),
        ])
        .env("HOME", stage)
        .env("TMPDIR", stage);
    if let Some(s) = sdk {
        cmd.env("SDKROOT", s);
    }
    cmd
}
pub struct Tools {
    pub node: PathBuf,
    pub rustc: PathBuf,
    pub clang: PathBuf,
    pub developer: Option<PathBuf>,
    pub sdk: Option<PathBuf>,
    pub privileged: bool,
    pub platform: V,
}
fn actual(c: &mut Command) -> Result<PathBuf> {
    Ok(Path::new(checked(c, 20)?.trim()).canonicalize()?)
}
impl Tools {
    pub fn detect(privileged: bool) -> Result<Self> {
        let locate = |s: &str| -> Result<PathBuf> {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|d| d.join(s))
                .find(|p| p.is_file())
                .context("required tool absent")?
                .canonicalize()
                .map_err(Into::into)
        };
        let rustc =
            actual(Command::new("rustup").args(["which", "--toolchain", "1.97.1", "rustc"]))?;
        let node = locate("node")?;
        let (developer, sdk, clang, platform) = if cfg!(target_os = "linux") {
            ensure!(
                unsafe { libc::getuid() } != 0,
                "Linux qualification requires regular user"
            );
            (
                None,
                None,
                locate("clang")?,
                json!({"bubblewrapSha256":sum(Path::new("/usr/bin/bwrap"))?,"privilegedMountSetup":privileged}),
            )
        } else {
            ensure!(
                cfg!(target_os = "macos") && !privileged,
                "unsupported sandbox host"
            );
            let d = actual(Command::new("xcode-select").arg("-p"))?;
            let sdk = actual(Command::new("xcrun").arg("--show-sdk-path"))?;
            let evidence = json!({"sdkSettingsSha256":sum(&sdk.join("SDKSettings.json"))?});
            (
                Some(d),
                Some(sdk),
                actual(Command::new("xcrun").args(["--find", "clang"]))?,
                evidence,
            )
        };
        Ok(Self {
            node,
            rustc,
            clang,
            developer,
            sdk,
            privileged,
            platform,
        })
    }
    pub fn boundary(
        &self,
        stage: &Path,
        read: &[&Path],
        executables: &[&Path],
    ) -> Result<Vec<String>> {
        if cfg!(target_os = "linux") {
            return Ok(crate::linux::boundary(
                stage,
                read,
                executables,
                self.privileged,
                unsafe { libc::getuid() },
                unsafe { libc::getgid() },
            )?);
        }
        let mut sub = vec![];
        for d in [
            "/System/Library",
            "/usr/lib",
            "/usr/bin",
            "/bin",
            "/Library/Apple",
            "/private/var/db/dyld",
            "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
            "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
        ]
        .map(Path::new)
        .into_iter()
        .chain([stage])
        .chain(read.iter().copied())
        {
            sub.push(format!("(subpath {})", serde_json::to_string(d)?));
        }
        let lit = executables
            .iter()
            .copied()
            .chain(["/", "/dev/null", "/dev/urandom"].map(Path::new))
            .map(|d| Ok(format!("(literal {})", serde_json::to_string(d)?)))
            .collect::<Result<Vec<_>>>()?;
        let policy = format!(
            "(version 1)\n(deny default)\n(allow process-fork process-exec file-read-metadata sysctl-read signal)\n(allow process-info* (target self))\n(allow file-read-data file-map-executable {} {})\n(allow file-write* (subpath {}) (literal \"/dev/null\"))\n",
            sub.join(" "),
            lit.join(" "),
            serde_json::to_string(stage)?
        );
        Ok(vec!["/usr/bin/sandbox-exec".into(), "-p".into(), policy])
    }
}
pub fn policy(prefix: &[String], stage: &Path) -> Result<String> {
    let s = if cfg!(target_os = "linux") {
        serde_json::to_string(prefix)?
    } else {
        prefix.last().context("policy")?.clone()
    };
    Ok(digest(
        s.replace(stage.to_str().context("stage")?, "$STAGE")
            .as_bytes(),
    ))
}
pub fn probes(prefix: &[String], stage: &Path, denied: &Denied) -> Result<Vec<String>> {
    if cfg!(target_os = "linux") {
        let mut c = command(prefix)?;
        environment(&mut c, stage, None).args(["/bin/cat", "/proc/self/status"]);
        let status = checked(&mut c, 10)?;
        let field = |k: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(k))
                .context("missing proc field")
        };
        ensure!(
            field("Uid:")?.split_whitespace().nth(1)
                == Some(&unsafe { libc::getuid() }.to_string())
                && u64::from_str_radix(field("CapEff:")?.trim(), 16)? == 0,
            "target retained privileges"
        );
    }
    let mut labels = vec![];
    for (label, path) in denied {
        ensure!(path.is_file(), "withheld input missing: {label}");
        let alias = PathBuf::from(format!("/System/Volumes/Data{}", path.display()));
        let mut variants = vec![(label.clone(), path)];
        if alias.is_file() {
            variants.push((format!("{label}ThroughDataVolume"), &alias));
        }
        for (label, path) in variants {
            let mut c = command(prefix)?;
            environment(&mut c, stage, None).arg("/bin/cat").arg(path);
            let (s, o, e) = run(&mut c, &[], 10)?;
            let diag = if cfg!(target_os = "linux") {
                "No such file or directory"
            } else {
                "Operation not permitted"
            };
            ensure!(
                s.code() == Some(1) && o.is_empty() && String::from_utf8_lossy(&e).contains(diag),
                "withheld probe failed: {label}"
            );
            labels.push(label);
        }
    }
    labels.sort();
    Ok(labels)
}
