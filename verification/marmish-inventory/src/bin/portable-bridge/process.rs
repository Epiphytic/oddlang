use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::process::{CommandExt, ExitStatusExt},
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
const LIMIT: usize = 131072;
const TIMEOUT: Duration = Duration::from_secs(10);
pub const NOFILE: u64 = 256;

pub fn limits() -> Result<Value> {
    let mut result = json!({});
    let record = |name: &str, resource, doc: &mut Value| -> Result<()> {
        let mut r = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(resource, &mut r) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        doc[name] = if name == "NOFILE" {
            json!([NOFILE, NOFILE])
        } else {
            json!([r.rlim_cur as i64, r.rlim_max as i64])
        };
        Ok(())
    };
    for (name, r) in [
        ("AS", libc::RLIMIT_AS),
        ("CORE", libc::RLIMIT_CORE),
        ("CPU", libc::RLIMIT_CPU),
        ("DATA", libc::RLIMIT_DATA),
        ("FSIZE", libc::RLIMIT_FSIZE),
        ("MEMLOCK", libc::RLIMIT_MEMLOCK),
        ("NOFILE", libc::RLIMIT_NOFILE),
        ("NPROC", libc::RLIMIT_NPROC),
        ("RSS", libc::RLIMIT_RSS),
        ("STACK", libc::RLIMIT_STACK),
    ] {
        record(name, r, &mut result)?;
    }
    #[cfg(target_os = "linux")]
    for (name, r) in [
        ("MSGQUEUE", libc::RLIMIT_MSGQUEUE),
        ("NICE", libc::RLIMIT_NICE),
        ("OFILE", libc::RLIMIT_NOFILE),
        ("RTPRIO", libc::RLIMIT_RTPRIO),
        ("RTTIME", libc::RLIMIT_RTTIME),
        ("SIGPENDING", libc::RLIMIT_SIGPENDING),
    ] {
        record(name, r, &mut result)?;
    }
    Ok(result)
}
pub struct Session(Child);
impl Drop for Session {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn watch(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}
fn poll(fds: &mut [libc::pollfd], until: Instant) -> Result<()> {
    loop {
        let remaining = until
            .checked_duration_since(Instant::now())
            .context("bridge response timeout")?;
        let status = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as _,
                remaining.as_millis().max(1) as i32,
            )
        };
        if status > 0 {
            return Ok(());
        }
        if status == 0 {
            bail!("bridge response timeout")
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}
fn consume<R: Read>(r: &mut R, buf: &mut Vec<u8>) -> Result<bool> {
    let mut chunk = [0; 8192];
    let n = r.read(&mut chunk)?;
    ensure!(buf.len() + n <= LIMIT, "bridge output exceeds bound");
    buf.extend_from_slice(&chunk[..n]);
    Ok(n > 0)
}
impl Session {
    pub fn start(binary: &Path, args: &[String], cwd: &Path) -> Result<Self> {
        let mut c = Command::new(binary);
        c.args(args)
            .current_dir(cwd)
            .env_clear()
            .envs([("PATH", "/usr/bin:/bin"), ("LANG", "C")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Child setup only uses async-signal-safe libc calls before exec.
        unsafe {
            c.pre_exec(|| {
                libc::umask(0o077);
                let r = libc::rlimit {
                    rlim_cur: NOFILE,
                    rlim_max: NOFILE,
                };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &r) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(Self(c.spawn()?))
    }
    pub fn exchange(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        self.0
            .stdin
            .as_mut()
            .context("closed stdin")?
            .write_all(input)?;
        let out = self.0.stdout.as_mut().context("closed stdout")?;
        let until = Instant::now() + TIMEOUT;
        let mut bytes = vec![];
        while !bytes.ends_with(b"\n") {
            poll(&mut [watch(out.as_raw_fd())], until)?;
            ensure!(
                consume(out, &mut bytes)?,
                "stdout closed before complete reply"
            );
        }
        Ok(bytes)
    }
    pub fn eof(mut self, input: &[u8]) -> Result<(i32, Vec<u8>, Vec<u8>)> {
        if !input.is_empty() {
            self.0
                .stdin
                .as_mut()
                .context("closed stdin")?
                .write_all(input)?;
        }
        drop(self.0.stdin.take());
        let until = Instant::now() + TIMEOUT;
        let mut out = self.0.stdout.take().context("stdout")?;
        let mut err = self.0.stderr.take().context("stderr")?;
        let mut channels = [watch(out.as_raw_fd()), watch(err.as_raw_fd())];
        let mut stdout = vec![];
        let mut stderr = vec![];
        while channels.iter().any(|c| c.fd >= 0) {
            poll(&mut channels, until)?;
            if channels[0].revents != 0 && !consume(&mut out, &mut stdout)? {
                channels[0].fd = -1;
            }
            if channels[1].revents != 0 && !consume(&mut err, &mut stderr)? {
                channels[1].fd = -1;
            }
        }
        loop {
            if let Some(s) = self.0.try_wait()? {
                let code = s
                    .code()
                    .or_else(|| s.signal().map(|signal| -signal))
                    .context("child status")?;
                return Ok((code, stdout, stderr));
            }
            ensure!(Instant::now() < until, "bridge failed to exit on EOF");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
