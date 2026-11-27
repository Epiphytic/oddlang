//! Exercise each ledger component through its generated JavaScript bindings in Node and Chromium.
use provider_validation::Result;
use std::{env, fs, path::Path, process::Command};

fn version(tool: &Path, expected: &str) -> Result {
    let output = Command::new(tool).arg("--version").output()?;
    if !output.status.success() || std::str::from_utf8(&output.stdout)?.trim_end() != expected {
        return Err(format!("expected {} version {expected}", tool.display()).into());
    }
    Ok(())
}

fn run(command: &mut Command) -> Result {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{command:?} exited with {status}").into());
    }
    Ok(())
}

fn main() -> Result {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [artifacts, out] = args.as_slice() else {
        return Err("usage: wasm-js-hosts ARTIFACTS REPORTS".into());
    };
    let artifacts = fs::canonicalize(artifacts)?;
    fs::create_dir_all(out)?;
    let reports = fs::canonicalize(out)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let js = root.join("target/wasm-js");
    let examples = root.join("examples/wasm/javascript");
    let node = Path::new("node");
    let jco = js.join("node_modules/.bin/jco");
    let tsc = js.join("node_modules/.bin/tsc");
    for name in ["package.json", "package-lock.json"] {
        if fs::read(js.join(name))? != fs::read(examples.join(name))? {
            return Err(format!("changed JavaScript dependency pin {name}").into());
        }
    }
    version(node, "v22.22.2")?;
    version(&jco, "1.42.0")?;
    version(&tsc, "Version 5.9.3")?;
    let runner = js.join("run.mjs");
    fs::copy(examples.join("run.mjs"), &runner)?;
    for implementation in ["manual", "generated"] {
        for world in ["ledger", "ledger-test"] {
            let name = format!("{implementation}-{world}");
            let guest = artifacts.join("guests").join(format!("{name}.wasm"));
            let binding = reports.join(name);
            run(Command::new(&jco)
                .arg("transpile")
                .arg(&guest)
                .args([
                    "--instantiation",
                    "async",
                    "--no-wasi-shim",
                    "--name",
                    "ledger",
                    "--out-dir",
                ])
                .arg(&binding))?;
            fs::write(binding.join("package.json"), "{\"type\":\"module\"}\n")?;
            let host = binding.join("host.ts");
            fs::copy(examples.join("host.ts"), &host)?;
            run(Command::new(&tsc)
                .args([
                    "--strict",
                    "--target",
                    "es2022",
                    "--module",
                    "esnext",
                    "--moduleResolution",
                    "bundler",
                ])
                .arg(host))?;
            run(Command::new(node)
                .arg(&runner)
                .arg(artifacts.join(world))
                .arg(guest)
                .arg(&binding)
                .arg(&binding))?;
        }
    }
    Ok(())
}
