use provider_validation::{Result, capture_command, pin, read, sha, subprocess, write};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
mod observed;
mod policies;

const LEAF: &str = "src/group_0/__projection_0.rs";

fn run(cmd: &mut Command, cwd: &Path, expected: Option<&str>) -> Result<Vec<u8>> {
    cmd.current_dir(cwd)
        .env("RUSTUP_TOOLCHAIN", "1.97.1")
        .env("CARGO_NET_OFFLINE", "true");
    let result = subprocess::run_with_timeout(cmd, None, Duration::from_secs(240))?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    match expected {
        None if result.status.success() => Ok(result.stdout),
        Some(msg) if !result.status.success() && stderr.contains(msg) => Ok(result.stdout),
        _ => Err(format!(
            "{cmd:?}: expected {expected:?}, received {}\n{stderr}",
            result.status
        )
        .into()),
    }
}
fn files(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        files.insert(
            entry
                .file_name()
                .into_string()
                .map_err(|_| "invalid artifact path")?,
            fs::read(entry.path())?,
        );
    }
    Ok(files)
}
struct Suite {
    cli: PathBuf,
    root: PathBuf,
    fixture: PathBuf,
    package: &'static str,
    host: String,
}
impl Suite {
    fn shape(&self, name: &str) -> Result {
        let target = self.package.replace('-', "_");
        let raw = run(
            capture_command()
                .arg(self.root.join("Cargo.toml"))
                .args([self.package, "lib", &target, &self.host, "default"])
                .env("ODD_RECOVERY_DATA_SHAPES", "1"),
            &self.root,
            None,
        )?;
        let value: Value = serde_json::from_slice(&raw)?;
        assert!(value.get("compiler").is_some());
        fs::write(self.root.join(name), raw)?;
        Ok(())
    }
    fn cli(&self, args: &[&str], out: &str, error: Option<&str>) -> Result {
        run(
            Command::new(&self.cli).args(args).arg(out),
            &self.root,
            error,
        )?;
        Ok(())
    }
    fn fresh(&self, args: &[&str], out: &str, error: Option<&str>) -> Result {
        self.cli(args, out, error)?;
        if self.root.join(out).exists() == error.is_some() {
            return Err(format!("unexpected publication state of {out}").into());
        }
        Ok(())
    }
    fn existing(&self, args: &[&str], out: &str) -> Result {
        let before = files(&self.root.join(out))?;
        self.cli(args, out, Some("publish fresh package"))?;
        assert_eq!(before, files(&self.root.join(out))?);
        Ok(())
    }
    fn manifest(&self, graph: &str, overlay: &str, module: &str) -> Result<Value> {
        Ok(
            json!({"format":"oddlang-native-package-v1","name":self.package,"version":"0.1.0","edition":"2021",
            "dependencies":{"zeroize":{"version":"1.9.1"}},"cargoLock":pin(&self.root,"Cargo.lock")?,
            "components":[{"kind":"buffered-input","module":module,"reexports":[],"graph":pin(&self.root,graph)?,"overlay":pin(&self.root,overlay)?}]}),
        )
    }
    fn assemble(&self, manifest: &Value, out: &str, error: Option<&str>) -> Result {
        write(&self.root.join("package.json"), manifest)?;
        self.fresh(
            &["build", "native-package", "rust", "package.json"],
            out,
            error,
        )
    }
    fn probe(&self, crate_dir: &Path, trace: Option<&Path>) -> Result {
        let mut cmd = Command::new("cargo");
        // All policy projections use the same package name and version.
        // Different target directories ensure Cargo compiles each distinct source.
        cmd.args(["test", "--locked", "--offline", "--quiet", "--target-dir"])
            .arg(crate_dir.join("target"));
        if let Some(path) = trace {
            cmd.env("ODD_BUFFER_TRACE", path);
        }
        let raw = run(&mut cmd, crate_dir, None)?;
        assert!(String::from_utf8(raw)?.contains("1 passed"));
        Ok(())
    }
    fn attach(&self, out: &str, probe: &str) -> Result {
        fs::OpenOptions::new()
            .append(true)
            .open(self.root.join(out).join(LEAF))?
            .write_all(probe.as_bytes())?;
        Ok(())
    }
}

fn main() -> Result {
    let args: Vec<_> = env::args().skip(1).collect();
    let [mode, cli] = args.as_slice() else {
        return Err("expected policies|observed ODDLANG".into());
    };
    let (family, package, source, verify): (_, _, _, fn(&Suite, &str) -> Result) =
        match mode.as_str() {
            "policies" => (
                "buffered-input",
                "light-meter",
                "fixture.rs",
                policies::verify,
            ),
            "observed" => (
                "observed-buffered-input",
                "field-scanner",
                "reference.rs",
                observed::verify,
            ),
            _ => return Err("expected policies|observed".into()),
        };
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().canonicalize()?;
    let version = String::from_utf8(run(Command::new("rustc").arg("-vV"), &root, None)?)?;
    let host = version
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("missing compiler host")?
        .to_owned();
    let s = Suite {
        cli: Path::new(cli).canonicalize()?,
        root,
        fixture: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(family),
        package,
        host,
    };
    let text = fs::read_to_string(s.fixture.join(source))?;
    fs::create_dir(s.root.join("src"))?;
    fs::write(s.root.join("src/lib.rs"), &text)?;
    fs::write(
        s.root.join("Cargo.toml"),
        format!(
            "[package]\nname=\"{package}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nzeroize=\"=1.9.1\"\n"
        ),
    )?;
    run(
        Command::new("cargo").args(["generate-lockfile", "--offline"]),
        &s.root,
        None,
    )?;
    verify(&s, &text)?;
    println!("Buffered-input {mode} native qualification passed");
    Ok(())
}
