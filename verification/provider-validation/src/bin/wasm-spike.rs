//! Exercise an import-free guest and compare three published component worlds with independent WIT.
use clap::Parser;
use provider_validation::{
    Result, sha,
    wasm_toolchain::{compiler, copy_tree, prepare, run},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    tools_cache: Option<PathBuf>,
    #[arg(long)]
    oddlang: Option<PathBuf>,
    #[arg(long)]
    empty_linker: Option<PathBuf>,
}

fn stdout(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .env("CARGO_NET_OFFLINE", "true")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "{cmd:?}: {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn check_wit(tools: &Path, binary: &Path, out: &Path, evidence: &Path, name: &str) -> Result {
    run(Command::new(tools).arg("validate").arg(binary))?;
    let wit = format!("{name}.wit");
    run(Command::new(tools)
        .args(["component", "wit"])
        .arg(binary)
        .arg("-o")
        .arg(out.join(&wit)))?;
    if fs::read(out.join(&wit))? != fs::read(evidence.join(&wit))? {
        return Err(format!("published interface changed: {wit}").into());
    }
    Ok(())
}

fn main() -> Result {
    let args = Args::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let cache = args
        .tools_cache
        .unwrap_or_else(|| root.join("target/wasm-tools"))
        .canonicalize()?;
    let temp = tempfile::Builder::new()
        .prefix("spike.")
        .tempdir_in(&cache)?;
    let out = temp.path();
    let (pins, tools) = prepare(&root, &cache, out)?;
    let toolchain = format!("+{}", pins.rust_version);
    let manifest = root.join("examples/wasm/Cargo.toml");
    let cargo_build = || {
        let mut cmd = Command::new("cargo");
        cmd.arg(&toolchain)
            .args(["build", "--locked", "--offline", "--manifest-path"])
            .arg(&manifest);
        cmd
    };
    run(cargo_build()
        .arg("--target-dir")
        .arg(out.join("guest"))
        .arg("--target")
        .arg(&pins.guest_target)
        .arg("--profile")
        .arg(&pins.guest_profile)
        .args(["-p", "pure-echo"]))?;
    let binary = out
        .join("guest")
        .join(&pins.guest_target)
        .join(&pins.guest_profile)
        .join("pure_echo.wasm");
    let evidence = root.join("examples/wasm/evidence");
    check_wit(&tools, &binary, out, &evidence, "pure-echo")?;
    let host = match args.empty_linker {
        Some(host) => host.canonicalize()?,
        None => {
            let target = root.join("target/wasm-host");
            run(cargo_build()
                .arg("--target-dir")
                .arg(&target)
                .args(["-p", "empty-linker"]))?;
            target.join("debug/empty-linker")
        }
    };
    let mut observations = stdout(Command::new("rustc").arg(&toolchain).arg("--version"))?;
    observations.push_str(&stdout(Command::new(&tools).arg("--version"))?);
    observations.push_str(&format!(
        "guest target = {} ({})\ncomponent sha256 = {}\nextracted WIT sha256 = {}\n",
        pins.guest_target,
        pins.guest_profile,
        sha(&binary)?,
        sha(&out.join("pure-echo.wit"))?
    ));
    let host_output = stdout(Command::new(host).arg(&binary))?;
    let reference = fs::read_to_string(evidence.join("observations.txt"))?;
    let expected = reference
        .lines()
        .skip_while(|line| !line.starts_with("resolved imports ="));
    if host_output.is_empty() || !host_output.lines().eq(expected) {
        return Err("empty-linker observations differ from the reference".into());
    }
    observations.push_str(&host_output);
    fs::write(out.join("observations.txt"), &observations)?;
    print!("{observations}");
    let store = out.join("cache");
    for (name, world) in [
        ("ledger", "ledger"),
        ("ledger-test", "ledger-test"),
        ("echo", "echo-world"),
    ] {
        let lock = if name == "ledger-test" {
            let ledger_key = format!("{:x}", Sha256::digest(b"urn:oddlang:ledger:wasm:package"));
            copy_tree(
                &out.join("ledger"),
                &store.join("wasm").join(ledger_key).join("1.0.0"),
            )?;
            "contracts/wasm/ledger-test/dependencies.lock.json"
        } else {
            "contracts/wasm/empty.lock.json"
        };
        let bundle = out.join(name);
        run(compiler(args.oddlang.as_deref(), &pins.rust_version)?
            .current_dir(&root)
            .args(["wasm", "publish"])
            .arg(format!("contracts/wasm/{name}"))
            .arg(lock)
            .arg(&store)
            .arg(&bundle))?;
        let core = out.join(format!("{name}-core.wasm"));
        let component = out.join(format!("{name}.wasm"));
        run(Command::new(&tools)
            .args(["component", "embed", "--dummy", "--world", world])
            .arg(bundle.join("wit"))
            .arg("-o")
            .arg(&core))?;
        run(Command::new(&tools)
            .args(["component", "new"])
            .arg(core)
            .arg("-o")
            .arg(&component))?;
        check_wit(&tools, &component, out, &evidence, name)?;
    }
    println!("Spike evidence: {}", temp.keep().display());
    Ok(())
}
