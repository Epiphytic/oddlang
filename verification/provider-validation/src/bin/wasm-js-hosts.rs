//! Exercise each ledger component through its generated JavaScript bindings in Node and Chromium.
use provider_validation::{Result, read, sha};
use serde_json::{Value, json};
use std::collections::BTreeMap;
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

fn modules(root: &Path, dir: &Path, hashes: &mut BTreeMap<String, String>) -> Result {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            modules(root, &path, hashes)?;
        } else if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("wasm" | "js" | "ts")
        ) {
            let name = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("invalid module path")?
                .replace('\\', "/");
            hashes.insert(name, sha(&path)?);
        }
    }
    Ok(())
}

fn observe(runner: &Path, bundle: &Path, guest: &Path, binding: &Path) -> Result {
    let manifest_path = bundle.join("manifest.json");
    let manifest = read(&manifest_path)?;
    let release = sha(&manifest_path)?;
    let workspace = read(&bundle.join("workspace.lock.json"))?;
    let pin = workspace["pins"]
        .as_array()
        .ok_or("missing workspace pins")?
        .iter()
        .find(|p| p["identity"] == manifest["identity"])
        .ok_or("missing manifest pin")?;
    if pin["manifestSha256"] != release {
        return Err("manifest pin mismatch".into());
    }
    let corpus = bundle.join("cases.json");
    if manifest["files"]["cases.json"] != sha(&corpus)? {
        return Err("corpus digest mismatch".into());
    }
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(corpus)?)?;
    let mut expected = Vec::new();
    for case in &cases {
        let mut calls = Vec::new();
        for call in case["calls"].as_array().ok_or("missing calls")? {
            calls.push(json!({
                "function": call["function"], "arguments": call["arguments"], "result": call["expected"]
            }));
        }
        expected
            .push(json!({"caseId": case["caseId"], "imports": case["imports"], "calls": calls}));
    }
    let mut hashes = BTreeMap::new();
    modules(binding, binding, &mut hashes)?;
    let input = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(
        input.as_file(),
        &json!({"cases":cases,"names":hashes.keys().collect::<Vec<_>>()}),
    )?;
    let output = Command::new("node")
        .arg(runner)
        .arg(binding)
        .arg(input.path())
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let observations: [Value; 2] = serde_json::from_slice(&output.stdout)?;
    let driver = sha(runner)?;
    let component = sha(guest)?;
    for (observed, (host, version)) in observations
        .iter()
        .zip([("node", "v22.22.2"), ("chromium", "145.0.7632.6")])
    {
        if observed["host"] != host
            || observed["version"] != version
            || observed["cases"].as_array() != Some(&expected)
        {
            return Err(format!("{host} runtime version or corpus transcript differs").into());
        }
        let name = binding
            .file_name()
            .ok_or("missing binding name")?
            .to_str()
            .ok_or("invalid binding name")?;
        let report = json!({
            "participant": name,
            "roles": ["wasm.exporter", "wasm.importer"],
            "ruleSetVersion": manifest["ruleSetVersion"],
            "jcoVersion": "1.42.0",
            "hostDriverSha256": driver,
            "world": manifest["world"],
            "releaseSha256": release,
            "componentSha256": component,
            "modules": hashes,
            "host": host,
            "version": version,
            "cases": observed["cases"],
        });
        fs::write(
            binding.with_file_name(format!("{name}-{host}.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
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
    if read(&js.join("node_modules/@bytecodealliance/jco/package.json"))?["version"] != "1.42.0" {
        return Err("changed jco package version".into());
    }
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
            observe(&runner, &artifacts.join(world), &guest, &binding)?;
        }
    }
    Ok(())
}
