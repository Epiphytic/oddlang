//! Exercise generated river-gauge owners with independent Unix socket observations.
use provider_validation::{
    Result, cli, pin, read,
    subprocess::{run, run_with_timeout},
    write,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::Shutdown,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const GOOD: &[u8] = br#"{"valid":true,"reading":{"phase":"calibrating"},"problem":null}"#;
const BAD: &[u8] = br#"{"valid":false,"reading":null,"problem":"unrecognized sample"}"#;
const REQUEST: &[u8] = br#"{"action":"inspect"}"#;

fn checked(cmd: &mut Command) -> Result {
    let out = run(cmd, None)?;
    if !out.status.success() {
        return Err(format!(
            "{cmd:?} {}\n{}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    std::io::stdout().write_all(&out.stdout)?;
    Ok(())
}

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
impl Owner {
    fn start(program: &Path, endpoint: &Path) -> Result<Self> {
        let mut owner = Self(
            Command::new(program)
                .arg("--channel")
                .arg(endpoint)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
        );
        let until = Instant::now() + Duration::from_secs(5);
        while !fs::symlink_metadata(endpoint).is_ok_and(|m| m.file_type().is_socket()) {
            if owner.0.try_wait()?.is_some() || Instant::now() >= until {
                return Err(format!("owner failed to bind {}", endpoint.display()).into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(owner)
    }
    fn stop(mut self, signal: &str) -> Result {
        checked(Command::new("/bin/kill").args([signal, &self.0.id().to_string()]))?;
        let until = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(s) = self.0.try_wait()? {
                break s;
            }
            if Instant::now() >= until {
                return Err("owner failed to exit after signal".into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = vec![];
        let mut stderr = vec![];
        self.0.stdout.take().unwrap().read_to_end(&mut stdout)?;
        self.0.stderr.take().unwrap().read_to_end(&mut stderr)?;
        if !status.success() || !stdout.is_empty() || !stderr.is_empty() {
            return Err(format!(
                "unexpected owner exit {status}, stdout {stdout:?}, stderr {stderr:?}"
            )
            .into());
        }
        Ok(())
    }
}
fn framed(data: &[u8]) -> Vec<u8> {
    let mut wire = (data.len() as u32).to_be_bytes().to_vec();
    wire.extend_from_slice(data);
    wire
}
fn connection(path: &Path) -> Result<UnixStream> {
    let peer = UnixStream::connect(path)?;
    peer.set_read_timeout(Some(Duration::from_secs(3)))?;
    peer.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(peer)
}
fn receive(peer: &mut UnixStream) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    peer.take(133).read_to_end(&mut bytes)?;
    if bytes.len() > 132 {
        return Err("response exceeds frame bound".into());
    }
    Ok(bytes)
}
fn request(path: &Path, wire: &[u8], expected: &[u8]) -> Result {
    let mut peer = connection(path)?;
    peer.write_all(wire)?;
    peer.shutdown(Shutdown::Write)?;
    assert_eq!(
        receive(&mut peer)?,
        framed(expected),
        "request bytes {wire:?}"
    );
    Ok(())
}
fn rejected(program: &Path, endpoint: &Path) -> Result<std::process::Output> {
    run_with_timeout(
        Command::new(program).arg("--channel").arg(endpoint),
        None,
        Duration::from_secs(4),
    )
}
fn exercise(program: &Path, root: &Path) -> Result {
    let endpoint = root.join("pipe");
    let child = Owner::start(program, &endpoint)?;
    assert_eq!(
        fs::metadata(&endpoint)?.permissions().mode() & 0o7777,
        0o600
    );
    request(&endpoint, &framed(REQUEST), GOOD)?;
    for wire in [
        framed(br#"["inspect"]"#),
        framed(br#"{"action":"ignore"}"#),
        framed(br#"{"action":"inspect","duplicate":true}"#),
        framed(br#"{"action":"inspect","action":"inspect"}"#),
        129_u32.to_be_bytes().to_vec(),
        0_u32.to_be_bytes().to_vec(),
        [&5_u32.to_be_bytes()[..], b"{}"].concat(),
    ] {
        request(&endpoint, &wire, BAD)?;
    }
    {
        let mut one = connection(&endpoint)?;
        let mut two = connection(&endpoint)?;
        one.write_all(&[0])?;
        two.write_all(&[0])?;
        assert!(receive(&mut connection(&endpoint)?)?.is_empty());
        assert_eq!(receive(&mut one)?, framed(BAD));
        assert_eq!(receive(&mut two)?, framed(BAD));
    }
    let other = rejected(program, &endpoint)?;
    assert_eq!(other.status.code(), Some(1));
    assert_eq!(other.stderr, b"gauge endpoint occupied\n");
    request(&endpoint, &framed(REQUEST), GOOD)?;
    child.stop("-TERM")?;
    assert!(!endpoint.try_exists()?);
    let shared = root.join("shared");
    fs::create_dir(&shared)?;
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o755))?;
    let other = rejected(program, &shared.join("pipe"))?;
    assert_eq!(other.status.code(), Some(1));
    assert_eq!(other.stderr, b"gauge directory is not private\n");
    fs::write(&endpoint, b"owned by fixture")?;
    assert!(!rejected(program, &endpoint)?.status.success());
    assert_eq!(fs::read(&endpoint)?, b"owned by fixture");
    fs::remove_file(&endpoint)?;
    let child = Owner::start(program, &endpoint)?;
    fs::remove_file(&endpoint)?;
    fs::write(&endpoint, b"replacement")?;
    child.stop("-INT")?;
    assert_eq!(fs::read(&endpoint)?, b"replacement");
    fs::remove_file(&endpoint)?;
    let child = Owner::start(program, &endpoint)?;
    fs::remove_file(&endpoint)?;
    let _replacement = UnixListener::bind(&endpoint)?;
    let identity = fs::metadata(&endpoint)?.ino();
    child.stop("-TERM")?;
    assert_eq!(fs::metadata(&endpoint)?.ino(), identity);
    Ok(())
}
fn package(root: &Path) -> Result<Value> {
    Ok(
        json!({"format":"oddlang-native-package-v1","name":"river-gauge","version":"1.0.0","edition":"2021","dependencies":{
        "clap":{"version":"4.6.7"},"libc":{"version":"0.2.189"},"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.151"},
        "tokio":{"version":"1.54.1","features":["io-util","macros","net","rt","signal","sync","time"]}},
        "cargoLock":pin(root,"Cargo.lock")?,"components":[{"kind":"local-status-owner","module":"owner","graph":pin(root,"graph.json")?,
        "reexports":["run_owner","BoundOwner","SnapshotPublisher","OwnerError"]}],"binaries":[{"name":"river-gauge","entry":["run_owner"]}]}),
    )
}
fn generate(
    cli: &Path,
    root: &Path,
    name: &str,
    binding: &Value,
    manifest: &mut Value,
) -> Result<PathBuf> {
    write(&root.join("rust.json"), binding)?;
    manifest["components"][0]["overlay"] = pin(root, "rust.json")?;
    let input = root.join("package.json");
    write(&input, manifest)?;
    let dest = root.join(name);
    checked(
        Command::new(cli)
            .args(["build", "native-package", "rust"])
            .arg(input)
            .arg(&dest),
    )?;
    Ok(dest)
}
fn consumer(fixture: &Path, root: &Path, build: &Path, name: &str) -> Result {
    let tests = build.join("tests");
    fs::create_dir(&tests)?;
    let file = format!("{name}.rs");
    fs::copy(fixture.join(&file), tests.join(file))?;
    checked(
        Command::new("cargo")
            .current_dir(build)
            .args(["test", "--release", "--locked", "--offline", "--test", name])
            .env("GAUGE_TEST_DIR", root)
            .env("CARGO_TARGET_DIR", build.join("target")),
    )
}
fn reject_graph(cli: &Path, root: &Path, graph: &Value, expected: &str) -> Result {
    let input = root.join("mutant.json");
    write(&input, graph)?;
    let out = root.join("invalid");
    let proc = run_with_timeout(
        Command::new(cli)
            .args(["build", "local-status-owner", "rust"])
            .arg(input)
            .arg(root.join("rust.json"))
            .arg(&out),
        None,
        Duration::from_secs(5),
    )?;
    assert!(
        !proc.status.success() && String::from_utf8_lossy(&proc.stderr).contains(expected),
        "unexpected mutant result {proc:?}"
    );
    assert!(!out.try_exists()?);
    Ok(())
}
fn main() -> Result {
    let cli = cli()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../local-status-owner");
    let temp = tempfile::Builder::new()
        .prefix("gauge-")
        .tempdir_in("/tmp")?;
    let root = temp.path().canonicalize()?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    for file in ["graph.json", "Cargo.lock"] {
        fs::copy(fixture.join(file), root.join(file))?;
    }
    let mut binding = json!({"format":"oddlang-local-status-owner-rust-v1","executable":"river-gauge","socket_option":"channel"});
    let mut manifest = package(&root)?;
    let build = generate(&cli, &root, "generated", &binding, &mut manifest)?;
    checked(
        Command::new("cargo")
            .current_dir(&build)
            .args(["build", "--release", "--locked", "--offline"])
            .env("CARGO_TARGET_DIR", build.join("target")),
    )?;
    exercise(&build.join("target/release/river-gauge"), &root)?;
    consumer(&fixture, &root, &build, "consumer")?;
    binding["routed_requests"] = json!(true);
    let routed = generate(&cli, &root, "routed-build", &binding, &mut manifest)?;
    consumer(&fixture, &root, &routed, "routed_consumer")?;
    binding["async_route_deadline_ms"] = json!(100);
    let asynchronous = generate(&cli, &root, "async-build", &binding, &mut manifest)?;
    consumer(&fixture, &root, &asynchronous, "async_routed_consumer")?;
    let graph = read(&root.join("graph.json"))?;
    let mut mutant = graph.clone();
    mutant["lifecycle"]["transitions"][0]["from"] = json!("not-reachable");
    reject_graph(&cli, &root, &mutant, "unreachable local status transition")?;
    let mut mutant = graph;
    mutant["lifecycle"]["stopping"] = json!("z".repeat(128));
    reject_graph(
        &cli,
        &root,
        &mutant,
        "declared local status frame exceeds listener bound",
    )?;
    println!(
        "native gauge owner passed snapshot publication, malformed frame, concurrency, signal and ownership checks"
    );
    Ok(())
}
