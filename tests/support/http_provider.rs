//! Build a pinned provider and launch an isolated service for each fixture.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use tempfile::TempDir;

pub struct ProviderBuild {
    pub root: TempDir,
}
impl ProviderBuild {
    pub fn generate(project: &Path, pin: &Path, cache: &Path, overlay: &Path) -> Self {
        let root = tempfile::tempdir().unwrap();
        let example = Path::new("examples/http/provider");
        fs::create_dir(root.path().join("src")).unwrap();
        for file in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
            fs::copy(example.join(file), root.path().join(file)).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(["http", "generate"])
            .args([project, pin, cache, overlay, &root.path().join("generated")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self { root }
    }
    pub fn build(&self) -> PathBuf {
        let output = Command::new(env!("CARGO"))
            .current_dir(self.root.path())
            .args(["build", "--offline", "--locked", "--target-dir", "target"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.root.path().join("target/debug/oddlang-http-provider")
    }
}

pub struct Provider {
    child: Child,
    pub addr: SocketAddr,
}
impl Drop for Provider {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Provider {
    pub fn launch(binary: &Path, fixture: &Value) -> Self {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        serde_json::to_writer(&mut file, fixture).unwrap();
        let mut command = Command::new(binary);
        command
            .arg("--fixture")
            .arg(file.path())
            .args(["--listen", "127.0.0.1:0"]);
        Self::start(command, Duration::from_secs(10))
    }
    pub fn start(mut command: Command, timeout: Duration) -> Self {
        let mut child = command.stdout(Stdio::piped()).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut service = Self {
            child,
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx
            .recv_timeout(timeout)
            .expect("provider readiness timeout");
        service.addr = line
            .trim()
            .strip_prefix("listening ")
            .unwrap()
            .parse()
            .unwrap();
        assert!(service.addr.ip().is_loopback());
        service
    }
    pub fn exchange(&self, request: &Value) -> Value {
        let timeout = Duration::from_secs(5);
        let mut stream = TcpStream::connect_timeout(&self.addr, timeout).unwrap();
        stream.set_write_timeout(Some(timeout)).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        let body = STANDARD
            .decode(request["bodyBase64"].as_str().unwrap())
            .unwrap();
        write!(
            stream,
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
            request["method"].as_str().unwrap(),
            request["path"].as_str().unwrap(),
            self.addr,
            body.len()
        )
        .unwrap();
        if let Some(ct) = request["contentType"].as_str() {
            write!(stream, "Content-Type: {ct}\r\n").unwrap();
        }
        stream.write_all(b"\r\n").unwrap();
        stream.write_all(&body).unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let status: u16 = line
            .split_whitespace()
            .nth(1)
            .expect("HTTP response status")
            .parse()
            .unwrap();
        let mut media = None;
        loop {
            line.clear();
            assert!(
                reader.read_line(&mut line).unwrap() > 0,
                "incomplete response headers"
            );
            if line == "\r\n" {
                break;
            }
            let (key, value) = line.split_once(':').unwrap();
            if key.eq_ignore_ascii_case("content-type") {
                media = Some(value.trim().to_owned());
            }
        }
        let mut received = vec![];
        reader.read_to_end(&mut received).unwrap();
        json!({"request":request,"response":{"status":status,"contentType":media,"bodyBase64":STANDARD.encode(received)}})
    }
}
