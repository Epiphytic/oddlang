//! Qualify a generated field station using private fixtures and local IPC.
mod fixture;
mod owner;
use owner::{Owner, handshake, inspect};
use provider_validation::{Result, cli, read, subprocess::run_with_timeout, write};
use serde_json::json;
use std::{
    fs,
    net::TcpListener,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixListener,
        process::ExitStatusExt,
    },
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};
fn require(ok: bool, message: &str) -> Result {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn socket(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket())
}
fn reject(cmd: &mut Command, runtime: &Path, message: &str) -> Result {
    let o = run_with_timeout(cmd, None, Duration::from_secs(5))?;
    require(
        o.status.code() == Some(1)
            && o.stdout.is_empty()
            && o.stderr == format!("{message}\n").as_bytes()
            && !runtime.exists(),
        &format!(
            "unexpected outcome for {message}: {}",
            String::from_utf8_lossy(&o.stderr)
        ),
    )
}
fn main() -> Result {
    let cli = cli()?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../foreground-profile-owner");
    let temp = tempfile::Builder::new()
        .prefix("fs-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")?;
    let root = temp.path().canonicalize()?;
    let station = fixture::Station::create(&root)?;
    let (binary, package) = fixture::project(&root, &source, &cli)?;
    let relay = TcpListener::bind(("127.0.0.1", 0))?;
    relay.set_nonblocking(true)?;
    let url = format!("wss://{}", relay.local_addr()?);
    let uid = fs::metadata(&root)?.uid();
    let runtime = root
        .join("runtime")
        .join(format!("fs-{uid}-{}", "11".repeat(8)));
    let endpoint = runtime.join("port");
    let locator = runtime.join("locator.json");
    let cmd = || {
        let mut c = Command::new(&binary);
        c.current_dir(&root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("HOME", root.join("home"))
            .env("TMPDIR", root.join("runtime"))
            .arg("--attached")
            .arg("--data")
            .arg(root.join("state"))
            .args(["--route", "direct", "--relay", &url, "serve"]);
        c
    };
    fs::write(&station.signing, fixture::scalar(2))?;
    reject(&mut cmd(), &runtime, "identity mismatch")?;
    fs::write(&station.signing, fixture::scalar(1))?;
    fs::set_permissions(&station.backup, fs::Permissions::from_mode(0o644))?;
    reject(&mut cmd(), &runtime, "unsafe file")?;
    fs::set_permissions(&station.backup, fs::Permissions::from_mode(0o600))?;
    {
        let lease = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(station.profile.join("station.lck"))?;
        lease.try_lock()?;
        reject(&mut cmd(), &runtime, "already running")?;
    }
    {
        let mut owner = Owner::start(&mut cmd())?;
        let _held = handshake(&relay, &mut owner)?;
        require(socket(&endpoint), "expected published status socket")?;
        owner.kill()?;
        let (status, _, _) = owner.finish()?;
        require(
            status.signal() == Some(9) && socket(&endpoint),
            "abrupt exit did not preserve stale socket",
        )?;
    }
    let binding = json!({"version":4,"token":"11".repeat(8),"profile":station.profile});
    for _ in 0..2 {
        let mut owner = Owner::start(&mut cmd())?;
        // A stale predecessor can leave the socket visible while this owner recreates it.
        let _held = handshake(&relay, &mut owner)?;
        let end = Instant::now() + Duration::from_secs(6);
        while !socket(&endpoint) {
            owner.running()?;
            require(Instant::now() < end, "no status socket")?;
            thread::sleep(Duration::from_millis(10));
        }
        require(read(&locator)? == binding, "runtime locator differs")?;
        inspect(&endpoint)?;
        owner.stop()?;
        let (status, out, err) = owner.finish()?;
        require(
            status.success() && out.is_empty() && err.is_empty() && !runtime.exists(),
            "station shutdown failed",
        )?;
    }
    {
        let mut owner = Owner::start(&mut cmd())?;
        let _held = handshake(&relay, &mut owner)?;
        let (status, out, err) = owner.finish()?;
        require(
            status.code() == Some(1)
                && out.is_empty()
                && err == b"activation deadline\n"
                && !runtime.exists(),
            "station failed activation deadline expectation",
        )?;
    }
    drop(relay);
    reject(
        cmd().args(["--storage", "remote"]),
        &runtime,
        "backend mismatch",
    )?;
    fixture::private_dir(&runtime)?;
    write(&locator, &binding)?;
    fs::set_permissions(&locator, fs::Permissions::from_mode(0o600))?;
    {
        let _occupied = UnixListener::bind(&endpoint)?;
        fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600))?;
        let o = run_with_timeout(&mut cmd(), None, Duration::from_secs(5))?;
        require(
            o.status.code() == Some(1) && o.stderr == b"runtime collision\n" && socket(&endpoint),
            "occupied station endpoint was not preserved",
        )?;
    }
    fs::remove_file(endpoint)?;
    fs::remove_file(locator)?;
    fs::remove_dir(runtime)?;
    fixture::reject_invalid_links(&cli, &root, &package)?;
    println!(
        "field station passed credentials, explicit backend selection, lease, TLS, shutdown, stale recovery and occupied endpoint preservation"
    );
    Ok(())
}
