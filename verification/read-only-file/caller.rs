#[cfg(test)]
mod file;

#[cfg(test)]
#[derive(Debug)]
enum Fault {
    Io(std::io::Error),
    Rejected(&'static str),
}

#[cfg(test)]
impl From<std::io::Error> for Fault {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[test]
fn descriptor_order_and_content() {
    use file::{check_sample, open_sample, read_sample};
    use sha2::{Digest, Sha256};
    use std::{fs, os::{fd::AsRawFd, unix::fs::{PermissionsExt, symlink}}};

    let root = std::env::temp_dir().join(format!("bounded-inert-archive-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let sample = root.join("sample");
    let bytes = [7u8; 56];
    fs::write(&sample, bytes).unwrap();
    fs::set_permissions(&sample, fs::Permissions::from_mode(0o600)).unwrap();

    let digest = Sha256::digest(bytes).into();
    assert_eq!(read_sample(&sample, 56).unwrap(), bytes);
    assert!(matches!(read_sample(&sample, u64::MAX), Err(Fault::Rejected("read-overflow"))));
    let file = open_sample(&sample, 57).unwrap();
    assert_ne!(unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
    assert!(matches!(read_sample(&sample, 55), Err(Fault::Rejected("unsafe-file"))));
    assert!(matches!(check_sample(&sample, 57, 0o600, &digest, true),
        Err(Fault::Rejected("wrong-metadata"))));
    assert!(matches!(check_sample(&sample, 56, 0o644, &[0; 32], true),
        Err(Fault::Rejected("wrong-metadata"))));
    assert!(matches!(check_sample(&sample, 56, 0o644, &[0; 32], false),
        Err(Fault::Rejected("wrong-digest"))));
    check_sample(&sample, 56, 0o600, &digest, true).unwrap();
    check_sample(&sample, 56, 0o644, &digest, false).unwrap();
    let symbolic = root.join("symbolic");
    symlink(&sample, &symbolic).unwrap();
    assert!(matches!(open_sample(&symbolic, 56),
        Err(Fault::Io(error)) if error.raw_os_error() == Some(libc::ELOOP)));
    let hard = root.join("hard");
    fs::hard_link(&sample, &hard).unwrap();
    assert!(matches!(open_sample(&sample, 56), Err(Fault::Rejected("unsafe-file"))));
    assert!(matches!(open_sample(&root, u64::MAX), Err(Fault::Rejected("unsafe-file"))));
    fs::remove_dir_all(root).unwrap();
}
