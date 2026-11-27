//! Capture both streams without filling a pipe and enforce a two-minute execution limit.
use crate::Result;
use std::{
    fs::File,
    io::{PipeWriter, Read, Seek},
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};
pub fn run(command: &mut Command, sink: Option<PipeWriter>) -> Result<Output> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .stdout(stdout.try_clone()?)
        .stderr(match sink {
            Some(pipe) => pipe.into(),
            None => std::process::Stdio::from(stderr.try_clone()?),
        })
        .stdin(std::process::Stdio::null())
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(120) {
            child.kill()?;
            child.wait()?;
            return Err(format!("{command:?} exceeded 120 seconds").into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    fn bytes(file: &mut File) -> Result<Vec<u8>> {
        file.rewind()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    Ok(Output {
        status,
        stdout: bytes(&mut stdout)?,
        stderr: bytes(&mut stderr)?,
    })
}
