//! Replay exact bytes from the independent meter peer.
use provider_validation::Result;
use serde::Deserialize;
use std::{
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    unit: bool,
    exchanges: Vec<Exchange>,
    status: i32,
    stdout_hex: String,
    stderr_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    request: String,
    response: Option<String>,
}
const LIMIT: Duration = Duration::from_secs(3);
struct Client(Option<Child>);
impl Drop for Client {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Client {
    fn finish(&mut self) -> Result<Output> {
        let child = self.0.as_mut().ok_or("client already finished")?;
        let until = Instant::now() + LIMIT;
        while child.try_wait()?.is_none() {
            if Instant::now() >= until {
                return Err("meter client did not finish within 3s".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(self.0.take().unwrap().wait_with_output()?)
    }
}
fn accept(listener: &UnixListener) -> Result<UnixStream> {
    let deadline = Instant::now() + LIMIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(LIMIT))?;
                stream.set_write_timeout(Some(LIMIT))?;
                return Ok(stream);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(e) => return Err(e.into()),
        }
    }
}
fn framed(body: &[u8]) -> Vec<u8> {
    let mut wire = (body.len() as u32).to_be_bytes().to_vec();
    wire.extend_from_slice(body);
    wire
}
fn replay(case: &Case, binary: &Path, path: &Path) -> Result {
    let peer = UnixListener::bind(path)?;
    peer.set_nonblocking(true)?;
    let mut command = Command::new(binary);
    command.args(["read", "--port"]).arg(path);
    if case.unit {
        command.args(["--unit", "kelvin"]);
    }
    let mut client = Client(Some(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    ));
    let mut finished = None;
    for step in &case.exchanges {
        let mut stream = accept(&peer)?;
        let expected = framed(step.request.as_bytes());
        let mut bytes = vec![];
        (&stream)
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes != expected {
            return Err(format!("unexpected request frame {bytes:?}, expected {expected:?}").into());
        }
        if let Some(reply) = &step.response {
            stream.write_all(&framed(reply.as_bytes()))?;
            stream.shutdown(Shutdown::Write)?;
        } else {
            // Hold the connection open until the client's declared deadline fires.
            finished = Some(client.finish()?);
            break;
        }
    }
    let output = match finished {
        Some(o) => o,
        None => client.finish()?,
    };
    if output.status.code() != Some(case.status)
        || hex(&output.stdout) != case.stdout_hex
        || hex(&output.stderr) != case.stderr_hex
    {
        return Err(format!("unexpected output {output:?}").into());
    }
    drop(peer);
    fs::remove_file(path)?;
    Ok(())
}
fn hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}
pub fn verify(fixture: &Path, binary: &Path, path: &Path) -> Result<usize> {
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(fixture)?)?;
    for case in &cases {
        replay(case, binary, path).map_err(|e| format!("{}: {e}", case.name))?;
    }
    Ok(cases.len())
}
