//! Capture the inventory of a selected release-mode compilation with the pinned rustc driver.
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const PIN: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";
fn output(cmd: &mut Command) -> Result<String> {
    let result = cmd.output()?;
    if !result.status.success() {
        io::stderr().write_all(&result.stderr)?;
        return Err(format!("{cmd:?} exited {}", result.status).into());
    }
    Ok(String::from_utf8(result.stdout)?.trim_end().to_owned())
}
fn stderr(cmd: &mut Command) -> Result {
    // Cargo's verbose trace is the historical witness read by release-rustdoc.
    let status = cmd.stdout(Stdio::from(io::stderr())).status()?;
    if !status.success() {
        return Err(format!("{cmd:?} exited {status}").into());
    }
    Ok(())
}
fn cargo(path: &str, rustc: &str) -> Command {
    let mut cmd = Command::new(path);
    for (key, _) in env::vars_os() {
        let key_text = key.to_string_lossy();
        if matches!(
            key_text.as_ref(),
            "RUSTFLAGS"
                | "RUSTDOCFLAGS"
                | "RUSTC_BOOTSTRAP"
                | "RUSTC_WRAPPER"
                | "RUSTC_WORKSPACE_WRAPPER"
                | "CARGO_ENCODED_RUSTFLAGS"
                | "CARGO_ENCODED_RUSTDOCFLAGS"
                | "CARGO_BUILD_TARGET"
                | "CARGO_BUILD_RUSTFLAGS"
                | "CARGO_BUILD_RUSTDOCFLAGS"
                | "CARGO_BUILD_RUSTC_WRAPPER"
                | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
        ) || key_text.starts_with("CARGO_PROFILE_")
            || (key_text.starts_with("CARGO_TARGET_")
                && (key_text.ends_with("_RUSTFLAGS") || key_text.ends_with("_RUSTDOCFLAGS")))
        {
            cmd.env_remove(key);
        }
    }
    cmd.env("RUSTC", rustc);
    cmd
}
fn enabled(name: &str) -> bool {
    env::var_os(name).as_deref() == Some(OsStr::new("1"))
}
fn capture(args: &[OsString]) -> Result {
    let [manifest, pkg, kind, name, triple, defaults, features @ ..] = args else {
        return Err("expected manifest, package, lib|bin, target name, triple, default|no-default and optional features".into());
    };
    let manifest = Path::new(manifest).canonicalize()?;
    let root = manifest.parent().ok_or("missing manifest parent")?;
    let tool =
        |name: &str| output(Command::new("rustup").args(["which", "--toolchain", "1.97.1", name]));
    let rustc = tool("rustc")?;
    let cargo_path = tool("cargo")?;
    let info = output(Command::new(&rustc).arg("-vV"))?;
    if !info
        .lines()
        .any(|line| line.strip_prefix("commit-hash: ") == Some(PIN))
    {
        return Err("unexpected rustc build for 1.97.1".into());
    }
    let host = info
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .filter(|v| !v.is_empty())
        .ok_or("missing host triple")?;
    let sysroot = output(Command::new(&rustc).args(["--print", "sysroot"]))?;
    let temp = tempfile::Builder::new()
        .prefix("oddlang-rust-recovery.")
        .tempdir()?;
    let scratch = temp.path();
    let driver_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rust-recovery");
    let mut build = cargo(&cargo_path, &rustc);
    build
        .env("RUSTC_BOOTSTRAP", "oddlang_rust_recovery_driver")
        .env("RUSTFLAGS", "-C rpath=yes")
        .args([
            "build",
            "--quiet",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(driver_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(scratch.join("driver"))
        .args(["--target", host]);
    if enabled("ODD_RECOVERY_TYPE_EXPORTS") {
        build.args(["--features", "public-type-bindings"]);
    }
    stderr(&mut build)?;
    let mut check = cargo(&cargo_path, &rustc);
    let verbose = enabled("ODD_RECOVERY_PROFILE_TRACE");
    if verbose {
        check.env(
            "CARGO_ENCODED_RUSTFLAGS",
            format!("--remap-path-prefix={}=/oddlang-workspace", root.display()),
        );
    }
    let inventory = scratch.join("inventory.json");
    check
        .current_dir(root)
        .env(
            "RUSTC_WORKSPACE_WRAPPER",
            scratch
                .join("driver")
                .join(host)
                .join("debug/oddlang-rust-recovery-driver"),
        )
        .env("ODD_RECOVERY_PACKAGE", pkg)
        .env("ODD_RECOVERY_TARGET", name)
        .env("ODD_RECOVERY_KIND", kind)
        .env("ODD_RECOVERY_OUT", &inventory)
        .env("ODD_RECOVERY_SYSROOT", &sysroot)
        .env("ODD_RECOVERY_COMPILER", PIN)
        .args([
            "check",
            if verbose { "-vv" } else { "--quiet" },
            "--locked",
            "--offline",
            "--release",
            "--manifest-path",
        ])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(scratch.join("target"))
        .arg("--target")
        .arg(triple)
        .arg("--package")
        .arg(pkg);
    if kind == "lib" {
        check.arg("--lib");
    } else {
        check.arg("--bin").arg(name);
    }
    if defaults == "no-default" {
        check.arg("--no-default-features");
    }
    for feature in features {
        check.arg("--features").arg(feature);
    }
    stderr(&mut check)?;
    io::stdout().write_all(&fs::read(inventory)?)?;
    Ok(())
}
fn install_adapter() -> Result {
    // The recovery profile resolves driver source pins beside the executable.
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rust-recovery")
        .canonicalize()?;
    let destination = directory.join(format!("rust-capture{}", env::consts::EXE_SUFFIX));
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    fs::copy(env::current_exe()?, temporary.path())?;
    temporary.persist(&destination)?;
    println!("{}", destination.display());
    Ok(())
}
fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let result = if args.as_slice() == [OsStr::new("--install-adapter")] {
        install_adapter()
    } else if args.len() < 6
        || !matches!(args[2].to_str(), Some("lib" | "bin"))
        || !matches!(args[5].to_str(), Some("default" | "no-default"))
    {
        eprintln!(
            "usage: rust-capture --install-adapter | MANIFEST PACKAGE lib|bin TARGET TRIPLE default|no-default [FEATURE ...]"
        );
        return ExitCode::from(2);
    } else {
        capture(&args)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
