//! The peer checks the request body and EOF before sending the independent response.
use provider_validation::{Result, subprocess::run_with_timeout};
use serde::Deserialize;
use std::{
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::net::UnixListener,
    path::Path,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Case {
    socket: String,
    args: Vec<String>,
    request: String,
    reply_length: Option<u32>,
    reply_bytes: String,
    exit: i32,
    stdout: String,
    stderr: String,
}

const TIMEOUT: Duration = Duration::from_secs(3);

impl Case {
    pub fn request_size(&self) -> usize {
        self.request.len()
    }

    pub fn arguments(&self, root: &Path) -> Result<Vec<String>> {
        let root = root.to_str().ok_or("non-UTF8 fixture root")?;
        Ok(self
            .args
            .iter()
            .map(|arg| arg.replace("{root}", root))
            .collect())
    }
    fn serve(&self, listener: UnixListener, done: mpsc::Receiver<()>) -> io::Result<()> {
        listener.set_nonblocking(true)?;
        let start = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && start.elapsed() < TIMEOUT => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        let mut header = [0; 4];
        stream.read_exact(&mut header)?;
        let mut body = vec![0; self.request.len()];
        stream.read_exact(&mut body)?;
        if header != (body.len() as u32).to_be_bytes()
            || body != self.request.as_bytes()
            || stream.read(&mut [0])? != 0
        {
            return Err(io::Error::other(
                "unexpected request bytes or missing half-close",
            ));
        }
        if let Some(length) = self.reply_length {
            let header = length.to_be_bytes();
            stream.write_all(&header[..2])?;
            let mut rest = header[2..].to_vec();
            rest.extend_from_slice(self.reply_bytes.as_bytes());
            stream.write_all(&rest)?;
            if let Err(e) = stream.shutdown(Shutdown::Write)
                && e.kind() != io::ErrorKind::NotConnected
            {
                return Err(e);
            }
        }
        // A silent peer must stay connected until the client's own deadline fires.
        let _ = done.recv_timeout(Duration::from_secs(6));
        Ok(())
    }

    pub fn exchange(&self, root: &Path, client: &mut Command) -> Result {
        let path = root.join(self.socket.replace(
            "{runtime}",
            &format!(
                "meter-{}-1a2b",
                std::os::unix::fs::MetadataExt::uid(&fs::metadata(root)?)
            ),
        ));
        let listener = UnixListener::bind(&path)?;
        let (signal, done) = mpsc::channel();
        let result = thread::scope(|scope| -> Result {
            let peer = scope.spawn(move || self.serve(listener, done));
            client.args(self.arguments(root)?);
            let observed = observe(client, self.exit, &self.stdout, &self.stderr);
            drop(signal);
            peer.join().map_err(|_| "field-meter peer panicked")??;
            observed
        });
        fs::remove_file(path)?;
        result
    }
}

pub fn observe(client: &mut Command, exit: i32, stdout: &str, stderr: &str) -> Result {
    let result = run_with_timeout(client, None, Duration::from_secs(5))?;
    if result.status.code() != Some(exit)
        || result.stdout != stdout.as_bytes()
        || result.stderr != stderr.as_bytes()
    {
        return Err(format!(
            "{client:?}: expected {exit} {stdout:?} {stderr:?}, observed {result:?}"
        )
        .into());
    }
    Ok(())
}
