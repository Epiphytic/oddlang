mod evidence;
mod trace;
use clap::Parser;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const COMMIT: &str = "8925ea358a0f265ca61026aadc7ecc506c545cbe";

#[derive(Parser)]
#[command(
    about = "Collect a nightly Rust library definition inventory and classify dep-info coverage"
)]
struct Args {
    workspace: PathBuf,
    package: String,
    output: PathBuf,
    #[arg(long)]
    stable_receipt: Option<PathBuf>,
    #[arg(long)]
    bundle: Option<PathBuf>,
    #[arg(long)]
    bundle_sha256: Option<String>,
    #[arg(long)]
    matching_report: Option<PathBuf>,
    #[arg(long)]
    vendor: Option<PathBuf>,
    #[arg(long)]
    sandbox: Option<PathBuf>,
    #[arg(long)]
    features: Option<String>,
    #[arg(long)]
    no_default_features: bool,
    #[arg(long, env = "CARGO_HOME")]
    cargo_home: Option<PathBuf>,
}
fn string(path: &Path) -> Result<String> {
    Ok(path.to_str().ok_or("non-UTF-8 path")?.into())
}
fn sha(path: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    io::copy(&mut fs::File::open(path)?, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_reader(fs::File::open(path)?)?)
}

// Historic receipts use JSON ensure_ascii, including escaped UTF-16 pairs.
fn ascii(json: &str) -> String {
    let mut out = String::new();
    for c in json.chars() {
        if c < '\u{7f}' {
            out.push(c);
        } else {
            for n in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{n:04x}"));
            }
        }
    }
    out
}
fn tool(name: &str, args: &[&str]) -> Result<String> {
    let o = Command::new(name)
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !o.status.success() {
        return Err(format!("{name}: {}", String::from_utf8_lossy(&o.stderr)).into());
    }
    Ok(String::from_utf8(o.stdout)?.trim().into())
}
fn host(version: &str) -> Result<&str> {
    Ok(version
        .lines()
        .find_map(|s| s.strip_prefix("host: "))
        .ok_or("missing compiler host")?)
}
struct Runner {
    root: PathBuf,
    output: PathBuf,
    env: BTreeMap<String, String>,
    sandbox: Option<PathBuf>,
}
impl Runner {
    fn run(&self, args: &[String], name: &str, sandbox: bool) -> Result<String> {
        let mut cmd = if let Some(profile) = self.sandbox.as_ref().filter(|_| sandbox) {
            let mut c = Command::new("/usr/bin/sandbox-exec");
            c.arg("-f").arg(profile).arg(&args[0]);
            c
        } else {
            Command::new(&args[0])
        };
        let o = cmd
            .args(&args[1..])
            .current_dir(&self.root)
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null())
            .output()?;
        fs::write(self.output.join(format!("{name}.stdout")), &o.stdout)?;
        let stderr = self.output.join(format!("{name}.stderr"));
        fs::write(&stderr, o.stderr)?;
        if !o.status.success() {
            return Err(format!("{name} failed; see {}", stderr.display()).into());
        }
        Ok(String::from_utf8(o.stdout)?)
    }
}

fn collect(args: Args) -> Result<()> {
    let root = args.workspace.canonicalize()?;
    let destination = std::path::absolute(args.output)?;
    let output = destination
        .parent()
        .ok_or("missing output parent")?
        .canonicalize()?
        .join(destination.file_name().ok_or("invalid output name")?);
    if output.starts_with(&root) {
        return Err("place the inventory output outside the workspace".into());
    }
    let cargo_home = args
        .cargo_home
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .ok_or("missing cargo home")?
        .canonicalize()?;
    let vendor = args.vendor.map(|p| p.canonicalize()).transpose()?;
    let sandbox = args.sandbox.map(|p| p.canonicalize()).transpose()?;
    fs::create_dir(&output)?;
    let channel = include_str!("../rust-toolchain.toml")
        .lines()
        .find_map(|l| {
            l.strip_prefix("channel = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or("missing pinned toolchain")?;
    let cargo = PathBuf::from(tool("rustup", &["which", "--toolchain", channel, "cargo"])?)
        .canonicalize()?;
    let toolchain = cargo
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid Cargo path")?
        .to_owned();
    let rustc = string(&toolchain.join("bin/rustc"))?;
    let rustdoc = string(&toolchain.join("bin/rustdoc"))?;
    let target = output.join("target");
    let manifest = string(&root.join("Cargo.toml"))?;
    let remap = format!("--remap-path-prefix={}=/oddlang-workspace", root.display());
    let mut env: BTreeMap<String, String> = [
        ("HOME", string(&output.join("home"))?),
        ("CARGO_HOME", string(&cargo_home)?),
        ("CARGO_TARGET_DIR", string(&target)?),
        ("CARGO_NET_OFFLINE", "true".into()),
        ("CARGO_TERM_COLOR", "never".into()),
        ("RUSTC", rustc.clone()),
        ("RUSTDOC", rustdoc),
        ("PATH", format!("{}/bin:/usr/bin:/bin", toolchain.display())),
        ("CARGO_ENCODED_RUSTFLAGS", remap.clone()),
        ("CARGO_ENCODED_RUSTDOCFLAGS", remap),
        ("TMPDIR", string(&output.join("tmp"))?),
        ("LC_ALL", "C".into()),
        ("TZ", "UTC".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    let mut roots = vec![
        ("workspace", root.clone()),
        ("target", target.clone()),
        ("registry-cache", cargo_home.join("registry/src")),
        ("git-dependency", cargo_home.join("git/checkouts")),
        ("toolchain", toolchain.clone()),
    ];
    if let Some(v) = vendor {
        roots.insert(0, ("registry", v));
    }
    if cfg!(target_os = "macos") {
        let sdk = PathBuf::from(tool("xcrun", &["--show-sdk-path"])?).canonicalize()?;
        let clang = PathBuf::from(tool("xcrun", &["--find", "clang"])?).canonicalize()?;
        let key = host(&tool(&rustc, &["-vV"])?)?
            .to_uppercase()
            .replace('-', "_");
        env.insert(format!("CARGO_TARGET_{key}_LINKER"), string(&clang)?);
        env.insert("CC".into(), string(&clang)?);
        env.insert("CXX".into(), string(&clang.with_file_name("clang++"))?);
        env.insert("SDKROOT".into(), string(&sdk)?);
        roots.push(("sdk", sdk));
    }
    fs::create_dir(&env["HOME"])?;
    fs::create_dir(&env["TMPDIR"])?;
    let runner = Runner {
        root: root.clone(),
        output: output.clone(),
        env,
        sandbox,
    };
    let compiler = runner.run(&[rustc.clone(), "-vV".into()], "rustc", false)?;
    if !compiler.contains(&format!("commit-hash: {COMMIT}")) {
        return Err("wrong pinned nightly Rust compiler".into());
    }
    let cfg = runner.run(
        &[rustc.clone(), "--print".into(), "cfg".into()],
        "cfg",
        false,
    )?;
    let cargo = string(&cargo)?;
    let strings = |s: &[&str]| s.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let metadata = strings(&[
        &cargo,
        "metadata",
        "--locked",
        "--offline",
        "--no-deps",
        "--format-version",
        "1",
        "--manifest-path",
        &manifest,
    ]);
    let metadata: Value = serde_json::from_str(&runner.run(&metadata, "metadata", true)?)?;
    let package = metadata["packages"]
        .as_array()
        .and_then(|p| p.iter().find(|r| r["name"] == args.package))
        .ok_or("package is absent from workspace metadata")?;
    let library = package["targets"]
        .as_array()
        .and_then(|ts| {
            ts.iter().find(|t| {
                t["doc"] == true
                    && t["kind"].as_array().is_some_and(|ks| {
                        ks.iter().any(|k| {
                            matches!(
                                k.as_str(),
                                Some(
                                    "lib"
                                        | "rlib"
                                        | "dylib"
                                        | "proc-macro"
                                        | "cdylib"
                                        | "staticlib"
                                )
                            )
                        })
                    })
            })
        })
        .ok_or("expected a documentable package library")?;
    let mut flags = strings(&[
        "--locked",
        "--offline",
        "--manifest-path",
        &manifest,
        "-p",
        &args.package,
    ]);
    if let Some(features) = args.features.filter(|s| !s.is_empty()) {
        flags.extend(["--features".into(), features]);
    }
    if args.no_default_features {
        flags.push("--no-default-features".into());
    }
    let mut check = strings(&[&cargo, "check"]);
    check.extend(flags.clone());
    check.extend(strings(&["--all-targets", "--message-format=json", "-vv"]));
    let log = runner.run(&check, "check", true)?;
    let messages = log
        .lines()
        .filter(|l| l.starts_with("{\"reason\":"))
        .map(serde_json::from_str)
        .collect::<std::result::Result<Vec<Value>, _>>()?;
    let artifact = messages
        .iter()
        .find(|r| {
            r["reason"] == "compiler-artifact"
                && r["package_id"] == package["id"]
                && r["target"] == *library
                && r["profile"]["test"] == false
        })
        .ok_or("expected a documentable package library")?;
    let mut doc = strings(&[&cargo, "rustdoc"]);
    doc.extend(flags);
    doc.extend(strings(&[
        "--lib",
        "--",
        "-Z",
        "unstable-options",
        "--output-format",
        "json",
        "--document-private-items",
    ]));
    runner.run(&doc, "rustdoc", true)?;
    let doc_file = target.join("doc").join(format!(
        "{}.json",
        library["name"].as_str().ok_or("missing target name")?
    ));
    let mut producers = trace::Producers::new();
    for row in messages
        .iter()
        .filter(|r| r["reason"] == "build-script-executed")
    {
        producers.insert(
            PathBuf::from(
                row["out_dir"]
                    .as_str()
                    .ok_or("missing generated directory")?,
            ),
            trace::package_id(
                row["package_id"]
                    .as_str()
                    .ok_or("missing producer package ID")?,
                &root,
            ),
        );
    }
    let definitions = evidence::definitions(&read(&doc_file)?, &roots, &producers, &root)?;
    let mut counts = BTreeMap::new();
    for row in &definitions {
        *counts
            .entry(row["kind"].as_str().ok_or("missing definition kind")?)
            .or_insert(0) += 1;
    }
    let invocations = trace::invocations(
        &root,
        &target,
        &rustc,
        &roots,
        &producers,
        &fs::read_to_string(output.join("check.stderr"))?,
    )?;
    let manifest_path = Path::new(
        artifact["manifest_path"]
            .as_str()
            .ok_or("missing manifest path")?,
    )
    .strip_prefix(&root)?;
    let mut manifests = BTreeMap::new();
    for name in [
        Path::new("Cargo.toml"),
        Path::new("Cargo.lock"),
        manifest_path,
    ] {
        manifests.insert(string(name)?, sha(&root.join(name))?);
    }
    let package_id = artifact["package_id"]
        .as_str()
        .ok_or("missing package id")?;
    let mut target_cfg: Vec<_> = cfg.lines().collect();
    target_cfg.sort();
    let mut report = json!({
        "version": 1,
        "nightly": channel,
        "compiler": compiler.trim(),
        "cargo": runner.run(&[cargo, "-V".into()], "cargo", false)?.trim(),
        "rustdocFormat": 61,
        "crate": trace::package_id(package_id, &root),
        "configuration": {
            "features": artifact["features"],
            "targetCfg": target_cfg,
            "definitionScope": "nightly rustdoc library with private items",
            "inputScope": "nightly Cargo check all targets with selected features"
        },
        "coverage": {
            "definitions": "rustdoc-emitted inventory only",
            "signatureTypes": "resolved Rustdoc references with explicit qualified path or IDs",
            "callResolution": "unavailable in rustdoc JSON",
            "bodyTypes": "unavailable in rustdoc JSON",
            "macroExpansionOrigin": "unavailable in rustdoc JSON",
            "inactiveBranches": "outside the selected configuration; unresolved"
        },
        "manifests": manifests,
        "commands": [check, doc],
        "compilerInvocations": invocations,
        "definitionCounts": counts,
        "definitions": definitions,
        "rawRustdocSha256": sha(&doc_file)?,
        "workspaceRoot": root
    });
    match (args.bundle, args.bundle_sha256) {
        (Some(dir), Some(pin)) => {
            report["syntaxBundle"] = evidence::bundle(
                &dir,
                &pin,
                root.join(manifest_path)
                    .parent()
                    .ok_or("invalid package manifest")?,
            )?
        }
        (None, None) => (),
        _ => return Err("bundle verification requires --bundle and --bundle-sha256".into()),
    }
    if let Some(path) = args.stable_receipt {
        let stable = read(&path)?;
        if stable["version"] != 2 {
            return Err("expected stable stream baseline receipt version 2".into());
        }
        for (name, digest) in manifests {
            if stable["cargoInputs"].get(&name) != Some(&Value::String(digest)) {
                return Err(format!("stable manifest or lock changed: {name}").into());
            }
        }
        report["stableComparison"] = evidence::compare_stable(&report, &stable)?;
        report["stableComparison"]["receiptSha256"] = json!(sha(&path)?);
    }
    if let Some(path) = args.matching_report {
        let old = read(&path)?;
        if old["workspaceRoot"] == report["workspaceRoot"] {
            return Err("candidate and original workspace roots match".into());
        }
        for key in [
            "compiler",
            "cargo",
            "crate",
            "configuration",
            "manifests",
            "compilerInvocations",
            "definitionCounts",
            "definitions",
        ] {
            if old.get(key) != report.get(key) {
                return Err(format!("nightly candidate differs: {key}").into());
            }
        }
        report["matchingReportSha256"] = json!(sha(&path)?);
    }
    let path = output.join("rust-inventory.json");
    fs::write(
        &path,
        format!("{}\n", ascii(&serde_json::to_string_pretty(&report)?)),
    )?;
    println!("{}", path.display());
    Ok(())
}

fn main() -> Result<()> {
    if env::args_os()
        .nth(1)
        .is_some_and(|s| s == "--cargo-commands")
        && env::args_os().count() == 2
    {
        let mut log = String::new();
        io::stdin().read_to_string(&mut log)?;
        println!("{}", serde_json::to_string(&trace::commands(&log)?)?);
        return Ok(());
    }
    collect(Args::parse())
}

#[cfg(test)]
mod tests;
