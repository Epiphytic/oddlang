//! Build the ledger component matrix and compare the complete canonical WIT surface.
use clap::Parser;
use provider_validation::{
    Result,
    wasm_toolchain::{Pins, compiler, copy_tree, prepare, run},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser)]
struct Args {
    bundles: PathBuf,
    out: PathBuf,
    #[arg(long)]
    tools_cache: Option<PathBuf>,
    #[arg(long)]
    oddlang: Option<PathBuf>,
}

struct Build<'a> {
    pins: &'a Pins,
    tools: PathBuf,
    out: &'a Path,
    stage: &'a Path,
}

impl Build<'_> {
    fn component(
        &self,
        project: &Path,
        name: &str,
        features: &str,
        wit: &Path,
        world: &str,
    ) -> Result {
        let target = self.stage.join("target");
        run(Command::new("cargo")
            .arg(format!("+{}", self.pins.rust_version))
            .args(["build", "--locked", "--offline", "--manifest-path"])
            .arg(project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .arg("--target")
            .arg(&self.pins.ledger_guest_target)
            .arg("--profile")
            .arg(&self.pins.guest_profile)
            .arg("--features")
            .arg(features))?;
        let core = target
            .join(&self.pins.ledger_guest_target)
            .join(&self.pins.guest_profile)
            .join("ledger_guest.wasm");
        let artifact = self.out.join(format!("{name}.wasm"));
        let surface = self.out.join(format!("{name}.wit"));
        let reference_core = self.stage.join("reference-core.wasm");
        let reference = self.stage.join("reference.wasm");
        let reference_wit = self.stage.join("reference.wit");
        let tools = || Command::new(&self.tools);
        run(tools()
            .args(["component", "new"])
            .arg(core)
            .arg("-o")
            .arg(&artifact))?;
        run(tools().arg("validate").arg(&artifact))?;
        run(tools()
            .args(["component", "targets", "--world", world])
            .arg(wit)
            .arg(&artifact))?;
        run(tools()
            .args(["component", "wit"])
            .arg(&artifact)
            .arg("-o")
            .arg(&surface))?;
        run(tools()
            .args(["component", "embed", "--dummy", "--world", world])
            .arg(wit)
            .arg("-o")
            .arg(&reference_core))?;
        run(tools()
            .args(["component", "new"])
            .arg(reference_core)
            .arg("-o")
            .arg(&reference))?;
        run(tools()
            .args(["component", "wit"])
            .arg(reference)
            .arg("-o")
            .arg(&reference_wit))?;
        if fs::read(surface)? != fs::read(reference_wit)? {
            return Err(format!("{name}: canonical WIT differs from {world}").into());
        }
        Ok(())
    }
}

fn manifests(template: &Path, project: &Path) -> Result {
    fs::create_dir_all(project)?;
    for name in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(template.join(name), project.join(name))?;
    }
    Ok(())
}

fn main() -> Result {
    let args = Args::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let bundles = args.bundles.canonicalize()?;
    fs::create_dir_all(&args.out)?;
    let out = args.out.canonicalize()?;
    let stage = tempfile::tempdir_in(&out)?;
    let cache = args
        .tools_cache
        .unwrap_or_else(|| root.join("target/wasm-tools"));
    let (pins, tools) = prepare(&root, &cache, stage.path())?;
    let build = Build {
        pins: &pins,
        tools,
        stage: stage.path(),
        out: &out,
    };
    let template = root.join("examples/wasm/ledger-guests");
    let ledger = out.join("source/ledger");
    manifests(&template, &ledger)?;
    copy_tree(&template.join("src"), &ledger.join("src"))?;
    for name in ["ledger", "ledger-test"] {
        copy_tree(
            &bundles.join(name).join("wit"),
            &ledger.join("wit").join(name),
        )?;
    }
    let mut cli = compiler(args.oddlang.as_deref(), &pins.rust_version)?;
    let native = stage.path().join("native");
    run(cli
        .current_dir(&root)
        .args(["build", "examples/ledger/v1"])
        .arg(&native))?;
    fs::copy(native.join("account.rs"), ledger.join("src/account.rs"))?;
    for (name, features, package) in [
        ("manual-ledger", "", "ledger"),
        ("manual-ledger-test", "instrumented", "ledger-test"),
        ("generated-ledger", "generated", "ledger"),
        (
            "generated-ledger-test",
            "generated,instrumented",
            "ledger-test",
        ),
        ("trap", "trap", "ledger"),
        ("double-import", "double-import", "ledger"),
        ("max-exclusive", "max-exclusive", "ledger"),
        ("probe", "probe", "ledger"),
    ] {
        build.component(
            &ledger,
            name,
            features,
            &bundles.join(package).join("wit"),
            package,
        )?;
    }
    for (name, feature) in [
        ("echo", ""),
        ("echo-rename", "rename"),
        ("echo-tags", ""),
        ("echo-import", "seed"),
    ] {
        let wit = bundles.join(name).join("wit");
        if !wit.try_exists()? {
            continue;
        }
        let project = out.join("source").join(name);
        manifests(&template, &project)?;
        fs::create_dir_all(project.join("src"))?;
        fs::copy(template.join("matrix/echo.rs"), project.join("src/lib.rs"))?;
        copy_tree(&wit, &project.join("wit"))?;
        build.component(&project, name, feature, &wit, "echo-world")?;
    }
    Ok(())
}
