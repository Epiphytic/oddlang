//! Native observation matrix from a fresh Git clone in a disposable Seatbelt sandbox.
use super::*;
use std::{
    collections::BTreeMap,
    io,
    os::unix::process::{CommandExt, ExitStatusExt},
    process::Stdio,
    time::{Duration, Instant},
};
mod environment;
use environment::{configure, tool};
const TARGET: &str = "aarch64-apple-darwin";
const TIMEOUT: u64 = 900;
struct Environment {
    vars: BTreeMap<String, String>,
    policy: Vec<String>,
    uv: String,
    python: String,
}
fn args(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|s| s.to_string()).collect()
}
fn file_digest(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut fs::File::open(path)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}
fn binary(path: &Path) -> Result<Value> {
    Ok(json!({"sha256":file_digest(path)?,"size":fs::metadata(path)?.len()}))
}
fn runner_hash() -> String {
    let mut hash = Sha256::new();
    for bytes in [
        include_bytes!("mod.rs").as_slice(),
        include_bytes!("environment.rs"),
        include_bytes!("../lib.rs"),
        include_bytes!("../../Cargo.lock"),
    ] {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}
fn clone(upstream: &Path, source: &Path, revision: &str) -> Result<()> {
    let input = upstream.to_str().context("invalid upstream path")?;
    let dest = source.to_str().context("invalid source path")?;
    ensure!(
        tool("git", &["-C", input, "rev-parse", "HEAD"])? == revision,
        "upstream revision does not match pinned Marmish"
    );
    tool(
        "git",
        &[
            "-c",
            "init.templateDir=",
            "clone",
            "--no-local",
            "--quiet",
            "--no-checkout",
            input,
            dest,
        ],
    )?;
    tool(
        "git",
        &[
            "-C",
            dest,
            "-c",
            "core.hooksPath=/dev/null",
            "checkout",
            "--quiet",
            "--detach",
            revision,
        ],
    )?;
    Ok(())
}
struct Runner<'a> {
    tmp: &'a Path,
    report: &'a Path,
    env: Environment,
    state: Value,
}
impl<'a> Runner<'a> {
    fn setup(upstream: &Path, tmp: &'a Path, report: &'a Path, revision: &str) -> Result<Self> {
        let source = tmp.join("source");
        clone(upstream, &source, revision)?;
        let env = configure(tmp, report)?;
        let state = json!({
            "revision":revision,"tree":tool("git",&["-C",source.to_str().unwrap(),"rev-parse","HEAD^{tree}"])?,
            "host":TARGET,"osVersion":tool("sw_vers",&["-productVersion"])?,"tools":{},"commands":[],"binaries":{},
            "platforms":{TARGET:"observed","aarch64-unknown-linux-gnu":"unobserved","x86_64-unknown-linux-gnu":"unobserved"},
            "locks":file_hashes(&source,&["Cargo.lock","rust-toolchain.toml","tools/code-ontology/uv.lock"])?,
            "isolation":{
                "profileSha256":file_digest(&report.join("baseline-macos.sb"))?,
                "network":"Seatbelt denies IP networking; allows run-local Unix sockets",
                "workspace":"new local clone of pinned Git tree","target":"empty disposable build directory",
                "state":"disposable HOME, TMPDIR, Cargo and uv working directories"
            },
            "timeoutSeconds":TIMEOUT,"runnerSha256":runner_hash(),
            "flags":{"RUSTFLAGS":"","RUST_TEST_THREADS":"1","CARGO_NET_OFFLINE":"true","SDKROOT":env.vars["SDKROOT"],"LC_ALL":"C","TZ":"UTC","UV_OFFLINE":"true","CARGO_BUILD_TARGET":"host default"}
        });
        Ok(Self {
            tmp,
            report,
            env,
            state,
        })
    }
    fn save(&self) -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(self.report)?;
        serde_json::to_writer_pretty(file.as_file_mut(), &self.state)?;
        file.write_all(b"\n")?;
        file.persist(self.report.join("baseline.json"))?;
        Ok(())
    }
    fn observe(&mut self, name: &str, cmd: &[String], timeout: Duration) -> Result<i32> {
        let tmp = self.tmp;
        let capture = tmp.join("logs");
        let report = self.report.join("logs");
        let stdout = format!("{name}.stdout");
        let stderr = format!("{name}.stderr");
        let policy = &self.env.policy;
        println!("{name}");
        let mut process = Command::new(&policy[0]);
        process
            .args(&policy[1..])
            .args(cmd)
            .current_dir(tmp.join("source"))
            .env_clear()
            .envs(&self.env.vars)
            .stdin(Stdio::null())
            .stdout(fs::File::create(capture.join(&stdout))?)
            .stderr(fs::File::create(capture.join(&stderr))?);
        // Detach from the parent session. A timeout terminates every child of the command.
        unsafe {
            process.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = process.spawn()?;
        let start = Instant::now();
        let (status, timed_out) = loop {
            if let Some(done) = child.try_wait()? {
                break (done, false);
            }
            if start.elapsed() >= timeout {
                let pid = i32::try_from(child.id())?;
                if unsafe { libc::killpg(pid, libc::SIGKILL) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error.into());
                    }
                }
                break (child.wait()?, true);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let code = status
            .code()
            .unwrap_or_else(|| -status.signal().unwrap_or(1));
        for log in [&stdout, &stderr] {
            fs::copy(capture.join(log), report.join(log))?;
        }
        let portable: Vec<_> = cmd
            .iter()
            .map(|a| {
                a.replace(tmp.to_str().unwrap(), "$RUN")
                    .replace(&self.env.uv, "uv")
            })
            .collect();
        let entry = json!({"name":name,"argv":portable,"exitCode":code,"timedOut":timed_out,"status":if timed_out{"timed-out"}else if code==0{"passed"}else{"failed"},
            "stdout":format!("logs/{stdout}"),"stdoutSha256":file_digest(&report.join(stdout))?,
            "stderr":format!("logs/{stderr}"),"stderrSha256":file_digest(&report.join(stderr))?});
        self.state["commands"].as_array_mut().unwrap().push(entry);
        self.save()?;
        Ok(code)
    }
    fn command(&mut self, name: &str, cmd: &[&str]) -> Result<i32> {
        self.observe(name, &args(cmd), Duration::from_secs(TIMEOUT))
    }
    fn probes(&mut self) -> Result<()> {
        let uv = self.env.uv.clone();
        let python = self.env.python.clone();
        for (name, argv) in [
            ("rustc", args(&["rustc", "-vV"])),
            ("cargo", args(&["cargo", "-Vv"])),
            ("uv", args(&[&uv, "--version"])),
            ("python", args(&[&python, "--version"])),
            ("git", args(&["git", "--version"])),
            ("clang", args(&["clang", "--version"])),
            ("sdk", args(&["xcrun", "--show-sdk-version"])),
        ] {
            ensure!(
                self.observe(name, &argv, Duration::from_secs(TIMEOUT))? == 0,
                "failed to capture {name}; inspect logs/{name}.stderr"
            );
            self.state["tools"][name] =
                json!(fs::read_to_string(self.report.join(format!("logs/{name}.stdout")))?.trim());
        }
        let rustc = text(&self.state["tools"]["rustc"])?;
        ensure!(
            rustc.contains(TARGET) && rustc.contains("release: 1.97.1"),
            "toolchain/host mismatch"
        );
        self.command("cfg", &["rustc", "--print", "cfg", "--target", TARGET])?;
        self.command(
            "ontology",
            &[
                &uv,
                "run",
                "--locked",
                "--offline",
                "--python",
                &python,
                "--project",
                "tools/code-ontology",
                "python",
                "tools/code-ontology/index.py",
                "check",
                "--ref",
                "HEAD",
            ],
        )?;
        self.cargo("metadata", "metadata", &["--format-version", "1"])?;
        self.cargo(
            "installed-metadata",
            "metadata",
            &[
                "--format-version",
                "1",
                "--features",
                "marmish-native/installed-checks",
            ],
        )?;
        Ok(())
    }
    fn cargo(&mut self, name: &str, action: &str, rest: &[&str]) -> Result<i32> {
        let mut cmd = vec!["cargo", action, "--locked", "--offline"];
        cmd.extend_from_slice(rest);
        self.command(name, &cmd)
    }
    fn matrix(&mut self, installed: bool) -> Result<()> {
        self.probes()?;
        for (name, selection) in [
            ("workspace", vec!["--workspace"]),
            (
                "installed",
                vec!["-p", "marmish-native", "--features", "installed-checks"],
            ),
        ] {
            let mut flags = selection.clone();
            flags.push("--no-fail-fast");
            self.cargo(name, "test", &flags)?;
            let raw = fs::read_to_string(self.report.join(format!("logs/{name}.stdout")))?;
            let entry = self.state["commands"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap();
            let failed = regex::Regex::new(r"(?m)^test (\S+) \.\.\. FAILED$")?;
            let ignored = regex::Regex::new(r"(?m)^test (\S+) \.\.\. ignored(?:, (.*))?$")?;
            let summaries: Vec<_> = raw
                .lines()
                .filter(|l| l.starts_with("test result: "))
                .collect();
            let failures: Vec<_> = failed
                .captures_iter(&raw)
                .map(|m| m[1].to_owned())
                .collect();
            let ignores: Vec<_> = ignored
                .captures_iter(&raw)
                .map(|m| {
                    [
                        m[1].to_string(),
                        m.get(2).map(|v| v.as_str()).unwrap_or("").to_string(),
                    ]
                })
                .collect();
            entry["summaries"] = json!(summaries);
            entry["failedTests"] = json!(failures);
            entry["ignored"] = json!(ignores);
            let mut flags = selection;
            flags.extend(["--", "--ignored", "--list"]);
            self.cargo(&format!("{name}-ignored"), "test", &flags)?;
        }
        if self.cargo(
            "release",
            "build",
            &["--release", "-p", "marmish-native", "--bins"],
        )? == 0
        {
            let release = self.tmp.join("target/release");
            self.state["auxiliaryBinaries"]["marmish-release-tool"] =
                binary(&release.join("marmish-release-tool"))?;
            for bin in ["marmish", "marmishd", "marmish-worker"] {
                let path = release.join(bin);
                self.state["binaries"][bin] = binary(&path)?;
                for flag in ["version", "help"] {
                    self.command(
                        &format!("{bin}-{flag}"),
                        &[path.to_str().unwrap(), &format!("--{flag}")],
                    )?;
                }
            }
            if installed {
                let observation = candidate::installed::observe(
                    self.tmp,
                    self.report,
                    text(&self.state["revision"])?,
                    &self.env.policy,
                    &self.env.vars,
                )?;
                self.state
                    .as_object_mut()
                    .unwrap()
                    .extend(object(&observation)?.clone());
            }
        }
        self.save()
    }
}

pub fn run(source: &Path, report: &Path, revision: &str, observe_installed: bool) -> Result<()> {
    ensure!(
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
        "this runner requires a native Apple Silicon macOS host"
    );
    fs::create_dir_all(report)?;
    ensure!(
        fs::read_dir(report)?.next().is_none(),
        "output directory must be empty"
    );
    let tmp = tempfile::Builder::new()
        .prefix("m-")
        .tempdir_in("/private/tmp")?;
    let report = report.canonicalize()?;
    Runner::setup(source, tmp.path(), &report, revision)?.matrix(observe_installed)
}
#[cfg(test)]
mod tests;
