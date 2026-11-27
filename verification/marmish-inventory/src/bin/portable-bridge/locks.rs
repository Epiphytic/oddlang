use std::{fs, io, os::fd::AsRawFd, path::Path};
pub fn observe(path: &Path) -> io::Result<&'static str> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(cfg!(target_os = "linux"))
        .open(path)?;
    let fd = file.as_raw_fd();
    #[cfg(target_os = "linux")]
    let probe = |acquire| unsafe {
        libc::lockf(
            fd,
            if acquire {
                libc::F_TLOCK
            } else {
                libc::F_ULOCK
            },
            0,
        )
    };
    #[cfg(target_os = "macos")]
    let probe = |acquire| unsafe {
        libc::flock(
            fd,
            if acquire {
                libc::LOCK_EX | libc::LOCK_NB
            } else {
                libc::LOCK_UN
            },
        )
    };
    if probe(true) == -1 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock
            || cfg!(target_os = "linux") && err.raw_os_error() == Some(libc::EACCES)
        {
            return Ok("held");
        }
        return Err(err);
    }
    if probe(false) == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok("available")
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn observes_linux_open_file_description_lock_and_release() -> io::Result<()> {
        let lock = tempfile::NamedTempFile::new()?;
        assert_eq!(observe(lock.path())?, "available");
        let owner = fs::OpenOptions::new().write(true).open(lock.path())?;
        // The Linux engine holds an OFD write lock over the entire file.
        let mut range: libc::flock = unsafe { std::mem::zeroed() };
        range.l_type = libc::F_WRLCK as _;
        range.l_whence = libc::SEEK_SET as _;
        let rc = unsafe { libc::fcntl(owner.as_raw_fd(), libc::F_OFD_SETLK, &range) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        assert_eq!(observe(lock.path())?, "held");
        drop(owner);
        assert_eq!(observe(lock.path())?, "available");
        Ok(())
    }
}
