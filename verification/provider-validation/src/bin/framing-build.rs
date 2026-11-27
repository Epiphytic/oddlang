//! Build and qualify the two native/component pairs for the framing projection.
use clap::Parser;
use provider_validation::{
    Result,
    wasm_toolchain::{copy_tree, prepare, run},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    upstream: PathBuf,
    import: PathBuf,
    projection: PathBuf,
    output: PathBuf,
    mutant: PathBuf,
    #[arg(long)]
    tools_cache: Option<PathBuf>,
    /// Use an existing local Wasmtime framing host.
    #[arg(long)]
    host: Option<PathBuf>,
}

fn observer(repo: &Path, name: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo).args([
        "+1.97.1",
        "run",
        "--quiet",
        "--offline",
        "--locked",
        "--manifest-path",
        "verification/marmish-inventory/Cargo.toml",
        "--bin",
        name,
        "--",
    ]);
    cmd
}

fn record(cmd: &mut Command, dest: &Path) -> Result {
    cmd.env("RUSTUP_AUTO_INSTALL", "0")
        .stdout(fs::File::create(dest)?);
    run(cmd)
}

struct Builder<'a> {
    stage: &'a Path,
    template: PathBuf,
    tools: PathBuf,
    version: String,
    target: String,
    profile: String,
    out: &'a Path,
}

impl Builder<'_> {
    fn pair(&self, name: &str, projection: &Path) -> Result {
        let project = self.stage.join(name);
        let dest = self.out.join(name);
        fs::create_dir(&dest)?;
        fs::create_dir(&project)?;
        for file in ["Cargo.toml", "Cargo.lock", "batch.rs"] {
            fs::copy(self.template.join(file), project.join(file))?;
        }
        for dir in ["core", "consumer", "guest"] {
            copy_tree(&self.template.join(dir), &project.join(dir))?;
        }
        fs::copy(
            projection.join("frame.rs"),
            project.join("core/src/frame.rs"),
        )?;
        fs::create_dir_all(project.join("guest/wit"))?;
        let wit = projection.join("world.wit");
        fs::copy(&wit, project.join("guest/wit/world.wit"))?;
        // The projects share crate names and versions but contain different projected source.
        let target = project.join("target");
        let cargo = || {
            let mut cmd = Command::new("cargo");
            cmd.arg(&self.version)
                .args(["build", "--locked", "--offline", "--manifest-path"])
                .arg(project.join("Cargo.toml"))
                .arg("--target-dir")
                .arg(&target);
            cmd
        };
        run(cargo().args(["--release", "-p", "framing-consumer"]))?;
        fs::copy(
            target.join("release/framing-consumer"),
            dest.join("consumer"),
        )?;
        run(cargo()
            .arg("--target")
            .arg(&self.target)
            .arg("--profile")
            .arg(&self.profile)
            .args(["-p", "framing-guest"]))?;
        let component = dest.join("component.wasm");
        let extracted = dest.join("extracted.wit");
        let tools = || Command::new(&self.tools);
        run(tools()
            .args(["component", "new"])
            .arg(
                target
                    .join(&self.target)
                    .join(&self.profile)
                    .join("framing_guest.wasm"),
            )
            .arg("-o")
            .arg(&component))?;
        run(tools().arg("validate").arg(&component))?;
        run(tools()
            .args(["component", "targets", "--world", "framing"])
            .arg(&wit)
            .arg(&component))?;
        run(tools()
            .args(["component", "wit"])
            .arg(&component)
            .arg("-o")
            .arg(&extracted))?;
        if fs::read(extracted)? != fs::read(self.out.join("expected.wit"))? {
            return Err(format!("{name}: extracted framing WIT changed").into());
        }
        Ok(())
    }
}

fn main() -> Result {
    let args = Args::parse();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let upstream = args.upstream.canonicalize()?;
    let import = args.import.canonicalize()?;
    let projection = args.projection.canonicalize()?;
    let mutant = args.mutant.canonicalize()?;
    fs::create_dir_all(&args.output)?;
    let out = args.output.canonicalize()?;
    if fs::read_dir(&out)?.next().is_some() {
        return Err("framing evidence requires an empty output directory".into());
    }
    let stage = tempfile::tempdir_in(&out)?;
    let cache = args
        .tools_cache
        .unwrap_or_else(|| repo.join("target/wasm-tools"));
    let (pins, tools) = prepare(&repo, &cache, stage.path())?;
    let version = format!("+{}", pins.rust_version);
    record(
        Command::new("rustc").arg(&version).arg("-vV"),
        &out.join("rustc.txt"),
    )?;
    record(
        Command::new(&tools).arg("--version"),
        &out.join("wasm-tools.txt"),
    )?;
    run(observer(&repo, "frame-matrix")
        .arg("verify-inputs")
        .arg(&import)
        .arg(&projection)
        .arg(&mutant))?;
    run(observer(&repo, "framing-observer")
        .arg("collect")
        .arg(upstream)
        .arg(out.join("original")))?;
    let host = if let Some(host) = args.host {
        host.canonicalize()?
    } else {
        let target = repo.join("target/wasm-host");
        run(Command::new("cargo")
            .current_dir(&repo)
            .arg(&version)
            .args([
                "build",
                "--locked",
                "--offline",
                "--manifest-path",
                "examples/wasm/Cargo.toml",
                "--target-dir",
            ])
            .arg(&target)
            .args(["--bin", "framing-host"]))?;
        target.join("debug/framing-host")
    };
    let local_host = stage.path().join("host");
    fs::copy(host, &local_host)?;
    let template = repo.join("examples/wasm/framing");
    let core = stage.path().join("reference-core.wasm");
    let component = stage.path().join("reference.wasm");
    run(Command::new(&tools)
        .args(["component", "embed", "--dummy", "--world", "framing"])
        .arg(template.join("host.wit"))
        .arg("-o")
        .arg(&core))?;
    run(Command::new(&tools)
        .args(["component", "new"])
        .arg(core)
        .arg("-o")
        .arg(&component))?;
    run(Command::new(&tools)
        .args(["component", "wit"])
        .arg(component)
        .arg("-o")
        .arg(out.join("expected.wit")))?;
    let build = Builder {
        stage: stage.path(),
        template,
        tools,
        version,
        target: pins.ledger_guest_target,
        profile: pins.guest_profile,
        out: &out,
    };
    build.pair("regenerated", &projection)?;
    build.pair("mutant", &mutant)?;
    run(observer(&repo, "frame-matrix")
        .arg("compare")
        .arg(out)
        .arg(local_host)
        .arg(import)
        .arg(projection)
        .arg(mutant))
}
