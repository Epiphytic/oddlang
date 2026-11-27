//! Compare local wait diagnostics in isolated, network-denied macOS profiles.
use clap::Parser;
use provider_validation::{Result, sha, subprocess::run_with_timeout};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::{
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::Duration,
};

#[derive(Parser)]
struct Args {
    mainline: PathBuf,
    sealed_export: PathBuf,
    #[arg(long)]
    include_link_cases: bool,
    #[arg(long)]
    require_match: bool,
}

const CASES: [&str; 13] = [
    "missing-state",
    "missing-profile",
    "missing-config",
    "world-readable-state",
    "regular-file-state",
    "symlink-state",
    "world-readable-profile",
    "malformed-config",
    "world-readable-config",
    "symlink-profile",
    "regular-file-profile",
    "symlink-config",
    "hardlinked-config",
];
const ARGV: [&str; 10] = [
    "--key-storage",
    "file",
    "--no-autostart",
    "--state-dir",
    "<state>",
    "--identity",
    "audit",
    "wait",
    "--handle",
    "{}",
];

fn directory(path: &Path) -> Result {
    fs::create_dir(path)?;
    Ok(fs::set_permissions(
        path,
        fs::Permissions::from_mode(0o700),
    )?)
}

fn file(path: &Path, bytes: &[u8], mode: u32) -> Result {
    fs::write(path, bytes)?;
    Ok(fs::set_permissions(path, fs::Permissions::from_mode(mode))?)
}

fn fixture(root: &Path, case: &str) -> Result {
    let state = root.join("state");
    let profile = state.join("audit");
    let config = profile.join("config.json");
    let target = root.join("target");
    match case {
        "missing-state" => return Ok(()),
        "regular-file-state" => return file(&state, b"", 0o600),
        "symlink-state" => {
            directory(&target)?;
            return Ok(symlink(target, state)?);
        }
        _ => directory(&state)?,
    }
    match case {
        "missing-profile" => return Ok(()),
        "world-readable-state" => {
            return Ok(fs::set_permissions(
                state,
                fs::Permissions::from_mode(0o755),
            )?);
        }
        "regular-file-profile" => return file(&profile, b"", 0o600),
        "symlink-profile" => {
            directory(&target)?;
            return Ok(symlink(target, profile)?);
        }
        _ => directory(&profile)?,
    }
    match case {
        "missing-config" => Ok(()),
        "world-readable-profile" => Ok(fs::set_permissions(
            profile,
            fs::Permissions::from_mode(0o755),
        )?),
        "malformed-config" => file(&config, b"{", 0o600),
        "world-readable-config" => file(&config, b"{", 0o644),
        "symlink-config" => {
            file(&target, b"{", 0o600)?;
            Ok(symlink(target, config)?)
        }
        "hardlinked-config" => {
            file(&target, b"{", 0o600)?;
            Ok(fs::hard_link(target, config)?)
        }
        _ => Err(format!("unknown fixture {case}").into()),
    }
}

#[derive(Serialize, PartialEq)]
struct Observation {
    exit: i32,
    stdout: String,
    stderr: String,
}

fn observe(binary: &Path, case: &str) -> Result<Observation> {
    let temp = tempfile::tempdir_in("/private/tmp")?;
    let root = temp.path();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    fixture(root, case)?;
    let policy = format!(
        "(version 1)(allow default)(deny network*)(deny mach-lookup)(deny file-write*)(allow file-write* (subpath {}))",
        serde_json::to_string(root)?
    );
    let state = root.join("state");
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-p")
        .arg(policy)
        .arg(binary)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("TMPDIR", root)
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C");
    for arg in ARGV {
        command.arg(if arg == "<state>" {
            state.as_os_str()
        } else {
            arg.as_ref()
        });
    }
    let result = run_with_timeout(&mut command, None, Duration::from_secs(3))?;
    if !result.stdout.is_ascii() || !result.stderr.is_ascii() {
        return Err("non-ASCII diagnostic".into());
    }
    Ok(Observation {
        exit: result
            .status
            .code()
            .unwrap_or_else(|| -result.status.signal().unwrap()),
        stdout: String::from_utf8(result.stdout)?,
        stderr: String::from_utf8(result.stderr)?,
    })
}

#[derive(Serialize)]
struct Pair<T> {
    mainline: T,
    generated: T,
}
#[derive(Default)]
struct Cases(Vec<(&'static str, Pair<Observation>)>);
impl Serialize for Cases {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (case, pair) in &self.0 {
            map.serialize_entry(case, pair)?;
        }
        map.end()
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    platform: &'static str,
    workspace_lock_sha256: String,
    binary_sha256: Pair<String>,
    argv: [&'static str; 10],
    cases: Cases,
}
fn main() -> Result<ExitCode> {
    let args = Args::parse();
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("this fixture requires ARM64 macOS".into());
    }
    let original = args.mainline.canonicalize()?;
    let generated = args.sealed_export.join("marmish").canonicalize()?;
    let mut report = Report {
        platform: "aarch64-apple-darwin",
        workspace_lock_sha256: sha(&args.sealed_export.join("workspace.lock"))?,
        binary_sha256: Pair {
            mainline: sha(&original)?,
            generated: sha(&generated)?,
        },
        argv: ARGV,
        cases: Cases::default(),
    };
    let mut differences = vec![];
    for &case in &CASES[..if args.include_link_cases { 13 } else { 9 }] {
        let pair = Pair {
            mainline: observe(&original, case)?,
            generated: observe(&generated, case)?,
        };
        if pair.mainline != pair.generated {
            differences.push(case);
        }
        report.cases.0.push((case, pair));
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    if args.require_match && !differences.is_empty() {
        eprintln!("Different results for {}", differences.join(", "));
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}
