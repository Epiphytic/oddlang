use std::{collections::BTreeSet, fs, path::Path};
pub fn boundary(
    stage: &Path,
    reads: &[&Path],
    exes: &[&Path],
    privileged: bool,
    uid: u32,
    gid: u32,
) -> std::io::Result<Vec<String>> {
    let mut args = vec![];
    let add = |a: &mut Vec<String>, v: &[&str]| a.extend(v.iter().map(|s| s.to_string()));
    if privileged {
        add(&mut args, &["/usr/bin/sudo", "-n"]);
    }
    add(
        &mut args,
        &[
            "/usr/bin/bwrap",
            "--unshare-net",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
        ],
    );
    let dir = stage
        .to_str()
        .ok_or_else(|| std::io::Error::other("stage path"))?;
    for (k, v) in [
        ("PATH", "/usr/bin:/bin"),
        ("HOME", dir),
        ("TMPDIR", dir),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("TZ", "UTC"),
    ] {
        add(&mut args, &["--setenv", k, v]);
    }
    let mut ancestors = BTreeSet::new();
    for p in [stage]
        .into_iter()
        .chain(reads.iter().copied())
        .chain(exes.iter().copied())
    {
        for parent in p.ancestors().skip(1).filter(|q| q.parent().is_some()) {
            ancestors.insert((parent.components().count(), parent));
        }
    }
    for d in [Path::new("/usr"), Path::new("/etc")] {
        ancestors.insert((d.components().count(), d));
    }
    for (_, p) in ancestors {
        add(
            &mut args,
            &[
                "--perms",
                "0755",
                "--dir",
                p.to_str().ok_or_else(|| std::io::Error::other("path"))?,
            ],
        );
    }
    for name in [
        "/usr/bin",
        "/usr/lib",
        "/usr/lib64",
        "/usr/libexec",
        "/etc/alternatives",
        "/etc/ld.so.cache",
        "/bin",
        "/lib",
        "/lib64",
    ] {
        let p = Path::new(name);
        if p.is_symlink() {
            add(
                &mut args,
                &[
                    "--symlink",
                    fs::read_link(p)?
                        .to_str()
                        .ok_or_else(|| std::io::Error::other("system symlink"))?,
                    name,
                ],
            );
        } else if p.exists() {
            add(&mut args, &["--ro-bind", name, name]);
        }
    }
    add(&mut args, &["--dev", "/dev", "--proc", "/proc"]);
    for p in reads.iter().chain(exes) {
        let s = p
            .to_str()
            .ok_or_else(|| std::io::Error::other("mount path"))?;
        add(&mut args, &["--ro-bind", s, s]);
    }
    add(&mut args, &["--bind", dir, dir, "--chdir", dir, "--"]);
    if privileged {
        add(
            &mut args,
            &[
                "/usr/bin/setpriv",
                "--reuid",
                &uid.to_string(),
                "--regid",
                &gid.to_string(),
                "--clear-groups",
                "--no-new-privs",
                "--",
            ],
        );
    }
    Ok(args)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    #[ignore = "requires a local Linux bubblewrap qualification host"]
    fn local_read_and_credential_boundary() {
        let temp = Command::new("mktemp").arg("-d").output().unwrap();
        assert!(temp.status.success());
        let dir = Path::new(std::str::from_utf8(&temp.stdout).unwrap().trim());
        let stage = dir.join("stage");
        let private = dir.join("withheld");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("allowed"), "permitted").unwrap();
        fs::write(&private, "private").unwrap();
        let id = |flag| -> u32 {
            let out = Command::new("id").arg(flag).output().unwrap();
            assert!(out.status.success());
            std::str::from_utf8(&out.stdout)
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        let uid = id("-u");
        assert_ne!(uid, 0);
        let privileged = std::env::var("ODDLANG_LINUX_PRIVILEGED_MOUNTS").as_deref() == Ok("1");
        let argv = boundary(&stage, &[], &[], privileged, uid, id("-g")).unwrap();
        let cat = |p: &Path| {
            Command::new(&argv[0])
                .args(&argv[1..])
                .arg("/bin/cat")
                .arg(p)
                .output()
                .unwrap()
        };
        let allowed = cat(&stage.join("allowed"));
        assert!(
            allowed.status.success(),
            "{}",
            String::from_utf8_lossy(&allowed.stderr)
        );
        assert_eq!(allowed.stdout, b"permitted");
        let denied = cat(&private);
        assert_eq!(denied.status.code(), Some(1));
        assert!(
            denied.stdout.is_empty()
                && String::from_utf8_lossy(&denied.stderr).contains("No such file or directory")
        );
        let status = cat(Path::new("/proc/self/status"));
        assert!(status.status.success());
        let proc = String::from_utf8(status.stdout).unwrap();
        let field = |key: &str| {
            proc.lines()
                .find_map(|l| l.strip_prefix(key))
                .unwrap()
                .trim()
        };
        assert_eq!(
            field("Uid:")
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u32>()
                .unwrap(),
            uid
        );
        assert_eq!(u64::from_str_radix(field("CapEff:"), 16).unwrap(), 0);
        fs::remove_dir_all(dir).unwrap();
    }
}
