use super::{Result, require};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    net::{Shutdown, TcpListener, TcpStream},
    os::unix::net::UnixStream,
    path::Path,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};
const DEADLINE: Duration = Duration::from_secs(5);
pub struct Owner {
    child: Child,
    out: File,
    err: File,
}
impl Owner {
    pub fn start(cmd: &mut Command) -> Result<Self> {
        let out = tempfile::tempfile()?;
        let err = tempfile::tempfile()?;
        let child = cmd
            .stdout(out.try_clone()?)
            .stderr(err.try_clone()?)
            .spawn()?;
        Ok(Self { child, out, err })
    }
    pub fn kill(&mut self) -> Result {
        self.child.kill()?;
        Ok(())
    }
    pub fn stop(&self) -> Result {
        let status = Command::new("/bin/kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status()?;
        require(status.success(), "could not signal station")
    }
    pub fn finish(&mut self) -> Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
        let end = Instant::now() + DEADLINE;
        let status = loop {
            if let Some(s) = self.child.try_wait()? {
                break s;
            }
            require(Instant::now() < end, "station did not exit by deadline")?;
            thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = vec![];
        let mut stderr = vec![];
        self.out.rewind()?;
        self.err.rewind()?;
        self.out.read_to_end(&mut stdout)?;
        self.err.read_to_end(&mut stderr)?;
        Ok((status, stdout, stderr))
    }
    pub fn running(&mut self) -> Result {
        require(
            self.child.try_wait()?.is_none(),
            "station exited before socket became ready",
        )
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
pub fn handshake(relay: &TcpListener, owner: &mut Owner) -> Result<TcpStream> {
    let end = Instant::now() + DEADLINE;
    let mut stream = loop {
        match relay.accept() {
            Ok((s, addr)) => {
                require(addr.ip().is_loopback(), "expected a loopback peer")?;
                break s;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                owner.running()?;
                require(Instant::now() < end, "no station TLS connection")?;
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    };
    stream.set_read_timeout(Some(DEADLINE))?;
    let mut prefix = [0; 3];
    stream.read_exact(&mut prefix)?;
    require(
        prefix == [0x16, 0x03, 0x01],
        "unexpected TLS handshake record prefix",
    )?;
    Ok(stream)
}
pub fn inspect(path: &Path) -> Result {
    let mut conn = UnixStream::connect(path)?;
    let limit = Duration::from_secs(4);
    conn.set_read_timeout(Some(limit))?;
    conn.set_write_timeout(Some(limit))?;
    let req = br#"{"action":"inspect"}"#;
    conn.write_all(&(req.len() as u32).to_be_bytes())?;
    conn.write_all(req)?;
    conn.shutdown(Shutdown::Write)?;
    let mut header = [0; 4];
    conn.read_exact(&mut header)?;
    let n = u32::from_be_bytes(header);
    require(n > 0 && n <= 128, "out-of-bounds station reply")?;
    let mut body = vec![0; n as usize];
    conn.read_exact(&mut body)?;
    require(
        body == br#"{"valid":true,"reading":{"phase":"calibrating"},"problem":null}"#,
        "station inspection bytes differed",
    )?;
    require(conn.read(&mut [0])? == 0, "trailing station reply bytes")
}
