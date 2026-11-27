//! Reproduce two clean component builds and replay their cases in Wasmtime, Node and Chromium.
use clap::Parser;
use provider_validation::{
    Result, sha,
    wasm_toolchain::{prepare, publish_fixture, run},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    /// Only prepare the artifacts required by the Wasmtime integration test.
    #[arg(long)]
    prepare: Option<PathBuf>,
    /// Directory containing the archive pinned by examples/wasm/toolchain.toml.
    #[arg(long)]
    tools_cache: Option<PathBuf>,
    /// Replay already-built local qualifier executables instead of invoking Cargo.
    #[arg(long)]
    built_tools: Option<PathBuf>,
}

struct Gate {
    root: PathBuf,
    cache: PathBuf,
    built: Option<PathBuf>,
}

impl Gate {
    fn program(&self, version: &str, name: &str, verifier: bool) -> Command {
        let mut cmd = if let Some(dir) = &self.built {
            Command::new(dir.join(name))
        } else {
            let mut cargo = Command::new("cargo");
            cargo
                .arg(format!("+{version}"))
                .args(["run", "--quiet", "--locked", "--offline"]);
            if verifier {
                cargo.args([
                    "--manifest-path",
                    "verification/provider-validation/Cargo.toml",
                ]);
            }
            cargo.args(["--bin", name, "--"]);
            cargo
        };
        cmd.current_dir(&self.root)
            .env("npm_config_offline", "true");
        cmd
    }

    fn artifacts(&self, out: &Path) -> Result<String> {
        fs::create_dir_all(out)?;
        if fs::read_dir(out)?.next().is_some() {
            return Err(format!(
                "component verification needs an empty output directory: {}",
                out.display()
            )
            .into());
        }
        let out = out.canonicalize()?;
        let stage = tempfile::tempdir_in(&out)?;
        let (pins, tools) = prepare(&self.root, &self.cache, stage.path())?;
        let version = pins.rust_version;
        fs::rename(tools, out.join("wasm-tools"))?;
        for name in ["ledger", "ledger-test", "echo"] {
            publish_fixture(
                self.program(&version, "oddlang", false),
                &self.root,
                &out,
                name,
            )?;
        }
        run(self
            .program(&version, "publish-wasm-matrix", false)
            .arg(&out))?;
        let mut guests = self.program(&version, "wasm-guests", true);
        guests
            .arg(&out)
            .arg(out.join("guests"))
            .arg("--tools-cache")
            .arg(&self.cache);
        if let Some(dir) = &self.built {
            guests.arg("--oddlang").arg(dir.join("oddlang"));
        }
        run(&mut guests)?;
        let host = if let Some(dir) = &self.built {
            dir.join("runtime-host")
        } else {
            let target = self.root.join("target/wasm-host");
            run(Command::new("cargo")
                .current_dir(&self.root)
                .arg(format!("+{version}"))
                .args([
                    "build",
                    "--locked",
                    "--offline",
                    "--manifest-path",
                    "examples/wasm/Cargo.toml",
                    "--target-dir",
                ])
                .arg(&target)
                .args(["-p", "runtime-host"]))?;
            target.join("debug/runtime-host")
        };
        fs::copy(host, out.join("runtime-host"))?;
        Ok(version)
    }

    fn verify(&self) -> Result<PathBuf> {
        let temp = tempfile::Builder::new()
            .prefix("verification.")
            .tempdir_in(&self.cache)?;
        let root = temp.keep();
        eprintln!("Component verification directory: {}", root.display());
        let first = root.join("first");
        let second = root.join("second");
        let version = self.artifacts(&first)?;
        self.artifacts(&second)?;
        for name in [
            "ledger",
            "ledger-test",
            "ledger-probe",
            "echo",
            "echo-label",
            "echo-rename",
            "echo-tags",
            "echo-import",
        ] {
            if inventory(&first.join(name))? != inventory(&second.join(name))? {
                return Err(format!("{name}: clean builds differ").into());
            }
        }
        let mut test = if let Some(dir) = &self.built {
            Command::new(dir.join("wasm_runtime"))
        } else {
            let mut cmd = Command::new("cargo");
            cmd.arg(format!("+{version}")).args([
                "test",
                "--locked",
                "--offline",
                "--test",
                "wasm_runtime",
            ]);
            cmd
        };
        run(test
            .current_dir(&self.root)
            .env("ODDLANG_WASM_ARTIFACTS", &first)
            .env("ODDLANG_WASM_HOST", first.join("runtime-host"))
            .env("WASM_TOOLS", first.join("wasm-tools")))?;
        run(self
            .program(&version, "wasm-js-hosts", true)
            .arg(&first)
            .arg(root.join("hosts")))?;
        run(self.program(&version, "wasm-reports", true).arg(&root))?;
        Ok(root)
    }
}

fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, Option<String>>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Option<String>>) -> Result {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            let digest = if kind.is_dir() {
                visit(root, &path, files)?;
                None
            } else if kind.is_file() {
                Some(sha(&path)?)
            } else {
                return Err(format!("unexpected release entry {}", path.display()).into());
            };
            files.insert(path.strip_prefix(root)?.into(), digest);
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
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
    let built = args.built_tools.map(fs::canonicalize).transpose()?;
    let gate = Gate { root, cache, built };
    if let Some(out) = args.prepare {
        gate.artifacts(&out)?;
    } else {
        println!(
            "Verified Wasmtime, Node and Chromium evidence in {}",
            gate.verify()?.display()
        );
    }
    Ok(())
}
