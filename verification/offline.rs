//! Build a child command isolated from IP networking. Append the executable and
//! its arguments separately so every caller keeps its own cwd, environment and I/O.
use std::{io, process::Command};

pub fn command() -> io::Result<Command> {
    match std::env::consts::OS {
        "macos" => {
            let mut cmd = Command::new("/usr/bin/sandbox-exec");
            cmd.args(["-p", r#"(version 1)(allow default)(deny network-outbound (remote ip "*:*"))(deny network-inbound (local ip "*:*"))"#]);
            Ok(cmd)
        }
        "linux" => {
            let mut cmd = Command::new("unshare");
            cmd.args(["--user", "--map-root-user", "--net", "--"]);
            Ok(cmd)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no network-isolated verifier for this host",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        io::Write,
        net::{TcpListener, TcpStream},
        process::Stdio,
    };

    #[test]
    #[ignore = "child of the host isolation test"]
    fn connect_probe() {
        let addr = env::var("ODDLANG_ISOLATION_PROBE_ADDR").unwrap();
        let result = TcpStream::connect(addr);
        if env::var_os("ODDLANG_ISOLATION_PROBE_DENIED").is_some() {
            assert!(result.is_err(), "isolated child reached the host listener");
        } else {
            result.expect("positive control must reach host listener");
        }
        println!("child observation complete");
    }

    #[test]
    #[ignore = "requires the host network isolation backend"]
    fn host_isolation_and_process_io() -> io::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let exe = env::current_exe()?;
        let probe = format!(
            "{}::connect_probe",
            module_path!().split_once("::").unwrap().1
        );
        for isolated in [false, true] {
            let mut cmd = if isolated {
                let mut cmd = command()?;
                cmd.arg(&exe).env("ODDLANG_ISOLATION_PROBE_DENIED", "1");
                cmd
            } else {
                let mut cmd = Command::new(&exe);
                cmd.env_remove("ODDLANG_ISOLATION_PROBE_DENIED");
                cmd
            };
            let out = cmd
                .env(
                    "ODDLANG_ISOLATION_PROBE_ADDR",
                    listener.local_addr()?.to_string(),
                )
                .args(["--ignored", "--exact", &probe, "--nocapture"])
                .output()?;
            assert!(
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout).contains("child observation complete"),
                "{out:?}"
            );
        }
        let mut child = command()?
            .args([
                "/bin/sh",
                "-c",
                r#"cat; printf '%s' "$1" >&2; exit 37"#,
                "probe",
                "a b;$()",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let bytes: Vec<u8> = (0..=255).collect();
        child.stdin.take().unwrap().write_all(&bytes)?;
        let result = child.wait_with_output()?;
        assert_eq!(result.status.code(), Some(37));
        assert_eq!(result.stdout, bytes);
        assert_eq!(result.stderr, b"a b;$()");
        Ok(())
    }
}
