//! Provision the pinned HTTP consumers and verify the offline cross-language gate.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const SCHEMA: &str = "openapi-3.1-2022-10-07.json";
const SCHEMA_SHA: &str = "da01ba28852cac0de53893797cb8d1942bc3b05084f526dcc216717dec314ed0";
const CHROMIUM: &str = "(async()=>{const browser=await require('playwright').chromium.launch({headless:true});try {console.log(browser.version())}finally {await browser.close()}})().catch(e=>{console.error(e);process.exitCode=1})";
const HYBRID: &str = "target/http-hybrid-deps";
const TOOLS: &str = "target/http-tools";
const SETUP: &str =
    "cargo +1.96.1 run --locked --manifest-path verification/http-conformance/Cargo.toml -- setup";
const OFFLINE: [(&str, &str); 4] = [
    ("CARGO_NET_OFFLINE", "true"),
    ("GOTOOLCHAIN", "local"),
    ("GOPROXY", "off"),
    ("npm_config_offline", "true"),
];
struct Gate {
    root: PathBuf,
    offline: bool,
}
impl Gate {
    fn command(&self, tool: &str, cwd: &Path) -> Command {
        let mut cmd = Command::new(tool);
        cmd.current_dir(cwd);
        if self.offline {
            cmd.envs(OFFLINE);
        }
        cmd
    }
    fn run(&self, cwd: &Path, tool: &str, args: &[&str]) -> Result {
        let status = self
            .command(tool, cwd)
            .args(args)
            .status()
            .map_err(|e| format!("start {tool}: {e}"))?;
        if !status.success() {
            return Err(format!("{tool} {} failed ({status})", args.join(" ")).into());
        }
        Ok(())
    }
    fn version(&self, cwd: &Path, tool: &str, expected: &str, args: &[&str]) -> Result {
        let v = self
            .command(tool, cwd)
            .args(args)
            .output()
            .map_err(|e| format!("Required tool: {tool} ({expected}): {e}"))?;
        if !v.status.success() {
            return Err(format!(
                "Required tool: {tool} ({expected}): {}",
                String::from_utf8_lossy(&v.stderr).trim()
            )
            .into());
        }
        let stdout = String::from_utf8(v.stdout)?;
        let actual = stdout.trim_end_matches(['\r', '\n']);
        if !(actual == expected
            || actual
                .strip_prefix(expected)
                .is_some_and(|rest| rest.starts_with(' ')))
        {
            return Err(format!("{tool}: expected {expected}, found {actual}").into());
        }
        Ok(())
    }
    fn runtime(&self) -> Result {
        for (tool, pin, args) in [
            ("rustc", "rustc 1.96.1", &["--version"][..]),
            ("node", "v22.22.2", &["--version"]),
            ("npm", "10.9.7", &["--version"]),
            ("go", "go version go1.26.3", &["version"]),
        ] {
            self.version(&self.root, tool, pin, args)?;
        }
        self.run(&self.root, "cargo", &["--version"])
    }
    fn tools(&self) -> Result {
        let dir = self.root.join(TOOLS);
        self.version(
            &dir,
            path(&dir.join("node_modules/.bin/tsc"))?,
            "Version 5.9.3",
            &["--version"],
        )?;
        self.version(
            &dir,
            path(&dir.join("node_modules/.bin/playwright"))?,
            "Version 1.58.2",
            &["--version"],
        )?;
        self.version(
            &dir,
            "node",
            "8.20.0",
            &["-p", "require('ajv/package.json').version"],
        )?;
        self.version(&dir, "node", "145.0.7632.6", &["-e", CHROMIUM])
    }
    fn hybrid(&self) -> Result {
        let dir = self.root.join(HYBRID);
        self.version(
            &dir,
            path(&dir.join("node_modules/.bin/vite"))?,
            "vite/8.3.8",
            &["--version"],
        )?;
        self.version(
            &dir,
            path(&dir.join("node_modules/.bin/playwright"))?,
            "Version 1.58.2",
            &["--version"],
        )?;
        for pkg in ["react", "react-dom"] {
            self.version(
                &dir,
                "node",
                "19.2.7",
                &["-p", &format!("require('{pkg}/package.json').version")],
            )?;
        }
        Ok(())
    }
    fn schema(&self, path: &Path) -> Result {
        let bytes = fs::read(path)?;
        if format!("{:x}", Sha256::digest(bytes)) != SCHEMA_SHA {
            return Err(format!(
                "{} differs from the pinned OpenAPI 3.1 schema",
                path.display()
            )
            .into());
        }
        Ok(())
    }
    fn setup(&self) -> Result {
        self.runtime()?;
        let tools = self.root.join(TOOLS);
        install_inputs(&self.root.join("scripts/http-tools"), &tools)?;
        self.run(&tools, "npm", &["ci", "--ignore-scripts"])?;
        self.run(
            &tools,
            path(&tools.join("node_modules/.bin/playwright"))?,
            &["install", "chromium"],
        )?;
        self.tools()?;
        let schema = tempfile::NamedTempFile::new_in(&tools)?;
        self.run(
            &tools,
            "curl",
            &[
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "https://spec.openapis.org/oas/3.1/schema/2022-10-07",
                "--output",
                path(schema.path())?,
            ],
        )?;
        self.schema(schema.path())?;
        schema.persist(tools.join(SCHEMA))?;
        self.run(&self.root, "cargo", &["fetch", "--locked"])?;
        for manifest in [
            "examples/contracts/monorepo/native/Cargo.toml",
            "examples/http/provider/Cargo.toml",
            "examples/http/contract/shapes/v1/rust-runner/Cargo.toml",
        ] {
            self.run(
                &self.root,
                "cargo",
                &["fetch", "--locked", "--manifest-path", manifest],
            )?;
        }
        self.run(
            &self.root,
            "npm",
            &[
                "exec",
                "--yes",
                "--package=typescript@5.9.3",
                "--",
                "tsc",
                "--version",
            ],
        )?;
        let hybrid = self.root.join(HYBRID);
        install_inputs(&self.root.join("examples/http/hybrid"), &hybrid)?;
        self.run(&hybrid, "npm", &["ci", "--ignore-scripts"])?;
        self.hybrid()
    }
    fn pins(&self) -> Result {
        self.runtime()?;
        for required in [
            "target/http-tools/node_modules",
            "target/http-hybrid-deps/package-lock.json",
        ] {
            if !self.root.join(required).exists() {
                return Err(format!("Missing {required}. Run {SETUP} first.").into());
            }
        }
        self.tools()?;
        self.schema(&self.root.join(TOOLS).join(SCHEMA))?;
        let hybrid = self.root.join(HYBRID);
        for name in ["package.json", "package-lock.json"] {
            if fs::read(self.root.join("examples/http/hybrid").join(name))?
                != fs::read(hybrid.join(name))?
            {
                return Err(
                    format!("Hybrid {name} differs from setup pin; run setup again").into(),
                );
            }
        }
        self.run(&hybrid,"npm",&["ci","--offline","--ignore-scripts","--no-audit"]).map_err(|e|format!("Missing hybrid npm packages for the checked-in package-lock.json. Run setup again. {e}"))?;
        self.hybrid()
    }
    fn verify(&self) -> Result {
        if env::var_os("ODDLANG_TEST_DATABASE_URL").is_none_or(|v| v.is_empty()) {
            return Err("ODDLANG_TEST_DATABASE_URL is required for the pinned PostgreSQL ledger tests. See README.md.".into());
        }
        self.pins()?;
        let scratch = tempfile::Builder::new()
            .prefix("http-verify.")
            .tempdir_in(self.root.join("target"))?;
        let marker = scratch.path().join("start");
        fs::File::create(&marker)?;
        self.run(
            &self.root,
            "cargo",
            &[
                "test",
                "--locked",
                "--offline",
                "--",
                "--skip",
                "exercise_released_components_and_reject_counterexamples",
            ],
        )?;
        self.evidence(scratch.path(), &marker, None)
    }
    fn evidence(&self, scratch: &Path, marker: &Path, cli: Option<&Path>) -> Result {
        let run = |args: &[&str]| -> Result {
            if let Some(exe) = cli {
                self.run(&self.root, path(exe)?, args)
            } else {
                let mut cmd = vec!["run", "--quiet", "--locked", "--offline", "--"];
                cmd.extend(args);
                self.run(&self.root, "cargo", &cmd)
            }
        };
        let receipts = self.root.join("target/http-conformance");
        let started = fs::metadata(marker)?.modified()?;
        let verify = |bundle: &str, pin: &str, claim: &str, name: &str| -> Result {
            let evidence = receipts.join(format!("{name}.json"));
            if !fs::metadata(&evidence)
                .and_then(|m| m.modified())
                .is_ok_and(|time| time > started)
            {
                return Err(format!("Missing fresh evidence: {}", evidence.display()).into());
            }
            run(&[
                "http",
                "verify-evidence",
                bundle,
                pin,
                claim,
                path(&evidence)?,
            ])
        };
        let ledger =
            "contracts/cache/http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4";
        for v in ["v1", "v2"] {
            let bundle = format!("{ledger}/{v}");
            let pin = format!("examples/http/contract/{v}/workspace.lock.json");
            for (name, claim) in [
                ("provider", "examples/http/provider/claim.json"),
                ("typescript", "examples/http/clients/typescript/claim.json"),
            ] {
                verify(&bundle, &pin, claim, &format!("{name}-{v}"))?;
            }
        }
        let go = "examples/http/clients/go";
        let target_lock = format!("{go}/target.lock.json");
        let v1 = format!("{ledger}/v1");
        run(&["http", "verify-target-lock", go, &target_lock, &v1])?;
        let lock: Value = serde_json::from_slice(&fs::read(self.root.join(&target_lock))?)?;
        let go_lock = scratch.join("go.lock.json");
        fs::write(
            &go_lock,
            serde_json::to_vec(
                &json!({"pins":[lock.get("contract").ok_or("missing Go contract pin")?]}),
            )?,
        )?;
        verify(&v1, path(&go_lock)?, &format!("{go}/claim.json"), "go-v1")?;
        let shapes = "examples/http/contract/shapes/v1";
        for lang in ["rust", "typescript", "go"] {
            verify(
                "contracts/cache/http/d550a36a22266cc54b0f99128f2851810bac7a782e5bdd519239f260a1f7392e/v1",
                &format!("{shapes}/workspace.lock.json"),
                &format!("{shapes}/claim.json"),
                &format!("shapes-{lang}"),
            )?;
        }
        println!(
            "Verified provider, client and codec evidence under {}.",
            receipts.display()
        );
        Ok(())
    }
}
fn path(p: &Path) -> Result<&str> {
    p.to_str()
        .ok_or_else(|| format!("non-UTF8 path {}", p.display()).into())
}
fn install_inputs(src: &Path, dest: &Path) -> Result {
    fs::create_dir_all(dest)?;
    for name in ["package.json", "package-lock.json"] {
        fs::copy(src.join(name), dest.join(name))?;
    }
    Ok(())
}
fn main() -> Result {
    let args: Vec<_> = env::args().skip(1).collect();
    let [mode] = args.as_slice() else {
        return Err("usage: http-conformance setup|verify|pins".into());
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .canonicalize()?;
    let gate = Gate {
        root,
        offline: mode != "setup",
    };
    match mode.as_str() {
        "setup" => gate.setup(),
        "verify" => gate.verify(),
        "pins" => gate.pins(),
        _ => Err("usage: http-conformance setup|verify|pins".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_stale_or_missing_evidence_before_running_a_verifier() -> Result {
        let dir = tempfile::tempdir()?;
        let gate = Gate {
            root: dir.path().to_owned(),
            offline: true,
        };
        let receipts = dir.path().join("target/http-conformance");
        fs::create_dir_all(&receipts)?;
        let start = dir.path().join("start");
        fs::File::create(&start)?;
        // The identical timestamp tests the strict newer-than comparison.
        fs::hard_link(&start, receipts.join("provider-v1.json"))?;
        let verify = || {
            gate.evidence(
                dir.path(),
                &start,
                Some(Path::new("/missing-test-verifier")),
            )
            .unwrap_err()
            .to_string()
        };
        assert!(verify().contains("Missing fresh evidence"));
        fs::remove_file(receipts.join("provider-v1.json"))?;
        assert!(verify().contains("Missing fresh evidence"));
        Ok(())
    }

    #[test]
    fn distinguishes_version_suffixes_and_detects_schema_tampering() -> Result {
        let dir = tempfile::tempdir()?;
        let gate = Gate {
            root: dir.path().to_owned(),
            offline: true,
        };
        gate.version(dir.path(), "/bin/echo", "1.2", &["1.2 build"])?;
        assert!(
            gate.version(dir.path(), "/bin/echo", "1.2", &["1.20"])
                .is_err()
        );
        let schema = dir.path().join(SCHEMA);
        fs::write(&schema, b"{}")?;
        assert!(
            gate.schema(&schema)
                .unwrap_err()
                .to_string()
                .contains("differs")
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires newly emitted cross-language evidence and the qualified Oddlang CLI"]
    fn verify_real_consumer_evidence() -> Result {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .canonicalize()?;
        let gate = Gate {
            root,
            offline: true,
        };
        let marker = PathBuf::from(
            env::var_os("ODDLANG_HTTP_STARTED").ok_or("ODDLANG_HTTP_STARTED required")?,
        );
        let cli =
            PathBuf::from(env::var_os("ODDLANG_HTTP_CLI").ok_or("ODDLANG_HTTP_CLI required")?);
        let scratch = tempfile::tempdir()?;
        gate.evidence(scratch.path(), &marker, Some(&cli))
    }
}
