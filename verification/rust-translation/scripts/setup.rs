//! Install the pinned proof tools before running offline translation verification.
use provider_validation::Result;
use provider_validation::proof_tools::ProofTools;
use std::{fs, process::Command};

fn checked(cmd: &mut Command) -> Result {
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("{cmd:?}: {status}").into());
    }
    Ok(())
}

fn main() -> Result {
    let tools = ProofTools::load()?;
    let list = tools
        .cargo()
        .args(["install", "--list", "--root"])
        .arg(&tools.prefix)
        .output()?;
    if !list.status.success() {
        return Err(String::from_utf8_lossy(&list.stderr).into_owned().into());
    }
    if !String::from_utf8(list.stdout)?
        .lines()
        .any(|s| s == "kani-verifier v0.68.0:")
    {
        checked(
            tools
                .cargo()
                .args(["install", "--locked", "--version", "=0.68.0", "--root"])
                .arg(&tools.prefix)
                .arg("kani-verifier"),
        )?;
    }
    let cbmc = tools.kani_home.join("bin/cbmc");
    let executable = match fs::metadata(cbmc) {
        Ok(info) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                info.is_file() && info.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                info.is_file()
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    if !executable {
        checked(tools.cargo().args(["kani", "setup"]))?;
    }
    let output = tools.cargo().args(["kani", "--version"]).output()?;
    let version = String::from_utf8(output.stdout)?;
    if !output.status.success()
        || !version
            .lines()
            .eq(["Kani Rust Verifier 0.68.0 (cargo plugin)", "CBMC 6.11.0"])
    {
        return Err(format!(
            "unexpected proof tools: {version} {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    print!("{version}");
    Ok(())
}
