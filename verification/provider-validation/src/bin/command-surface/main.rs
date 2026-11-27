use provider_validation::{Result, pin, read, subprocess, write};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
mod check;
mod prefix;
mod terminal;

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
impl Observation {
    fn exact(&self, status: i32, stdout: &[u8], stderr: &[u8]) {
        assert_eq!(
            (self.status, self.stdout.as_slice(), self.stderr.as_slice()),
            (status, stdout, stderr)
        );
    }
    fn stderr_has(&self, message: &str) -> bool {
        String::from_utf8_lossy(&self.stderr).contains(message)
    }
    fn stdout_has(&self, message: &str) -> bool {
        String::from_utf8_lossy(&self.stdout).contains(message)
    }
}
struct Suite {
    cli: PathBuf,
    root: PathBuf,
    fixture: PathBuf,
    timeout: Duration,
}
impl Suite {
    fn run(&self, cmd: &mut Command) -> Result<Observation> {
        cmd.env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env("RUSTUP_TOOLCHAIN", "1.97.1")
            .env("CARGO_NET_OFFLINE", "true");
        let result = subprocess::run_with_timeout(cmd, None, self.timeout)?;
        Ok(Observation {
            status: result.status.code().ok_or("process terminated by signal")?,
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }
    fn expect(&self, cmd: &mut Command, failure: Option<&str>) -> Result {
        let out = self.run(cmd)?;
        if match failure {
            None => out.status == 0,
            Some(msg) => out.status != 0 && out.stderr_has(msg),
        } {
            Ok(())
        } else {
            Err(format!(
                "{cmd:?}: expected {failure:?}, got {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            )
            .into())
        }
    }
    fn project(&self, graph: &Path, overlay: &Path, out: &Path, error: Option<&str>) -> Result {
        self.expect(
            Command::new(&self.cli)
                .args(["build", "cli-surface", "rust"])
                .arg(graph)
                .arg(overlay)
                .arg(out),
            error,
        )
    }
    fn recover(
        &self,
        route: &str,
        source: &Path,
        selection: &Path,
        out: &Path,
        error: Option<&str>,
    ) -> Result {
        self.expect(
            Command::new(&self.cli)
                .args(["recover", route])
                .arg(source)
                .arg(selection)
                .arg(out),
            error,
        )
    }
    fn recover_selected(&self, route: &str, source: &Path, selection: Value) -> Result<Value> {
        let select = self.root.join("selection.json");
        let graph = self.root.join("recovered.json");
        write(&select, &selection)?;
        self.recover(route, source, &select, &graph, None)?;
        read(&graph)
    }
    fn base(&self, out: &Path) -> Result {
        self.recover(
            "cli-surface",
            &self.fixture.join("src/bin/reference.rs"),
            &self.fixture.join("selection.json"),
            out,
            None,
        )
    }
    fn scaffold(&self, dir: &Path) -> Result {
        fs::create_dir_all(dir.join("src"))?;
        for file in ["Cargo.toml", "Cargo.lock"] {
            fs::copy(self.fixture.join(file), dir.join(file))?;
        }
        Ok(())
    }
    fn auxiliary(&self, source: &str) -> Result<PathBuf> {
        self.scaffold(&self.root)?;
        let bins = self.root.join("src/bin");
        fs::create_dir(&bins)?;
        fs::write(
            bins.join("candidate.rs"),
            "#[path=\"../cli.rs\"] mod cli;\nfn main() -> std::process::ExitCode { cli::run_cli() }\n",
        )?;
        let reference = bins.join("reference.rs");
        fs::write(&reference, source)?;
        Ok(reference)
    }
    fn cargo(&self, dir: &Path, action: &str) -> Result {
        self.expect(
            Command::new("cargo").current_dir(dir).args([
                action,
                "--locked",
                "--offline",
                "--target-dir",
                "target",
            ]),
            None,
        )
    }
    fn cases(&self, mode: &str) -> Result<Vec<Vec<String>>> {
        Ok(serde_json::from_slice(&fs::read(
            self.fixture.join(format!("{mode}-cases.json")),
        )?)?)
    }
    fn observe(&self, exe: &Path, args: &[&str]) -> Result<Observation> {
        self.run(Command::new(exe).args(args))
    }
    fn compare(&self, reference: &Path, candidate: &Path, args: &[&str]) -> Result<Observation> {
        let native = self.observe(reference, args)?;
        let actual = self.observe(candidate, args)?;
        assert_eq!(actual, native, "arguments {args:?}");
        Ok(actual)
    }
}
fn main() -> Result {
    let args: Vec<_> = env::args().skip(1).collect();
    let [mode, cli] = args.as_slice() else {
        return Err("expected check|prefix|terminal ODDLANG".into());
    };
    let scratch = tempfile::tempdir()?;
    let s = Suite {
        cli: Path::new(cli).canonicalize()?,
        root: scratch.path().canonicalize()?,
        fixture: Path::new(env!("CARGO_MANIFEST_DIR")).join("../command-surface"),
        timeout: Duration::from_secs(if mode == "terminal" { 90 } else { 120 }),
    };
    match mode.as_str() {
        "check" => check::verify(&s)?,
        "prefix" => prefix::verify(&s)?,
        "terminal" => terminal::verify(&s)?,
        _ => return Err("unknown command surface suite".into()),
    };
    println!("Command-surface {mode} native qualification passed");
    Ok(())
}
