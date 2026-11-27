//! The toolchain manifest contains quoted ASCII scalar assignments and comments.
use std::{collections::BTreeMap, fs, io, path::Path};

pub struct Pins {
    pub rust_version: String,
    pub rust_commit: String,
    pub rust_manifest_sha256: String,
    pub guest_target: String,
    pub ledger_guest_target: String,
    pub guest_profile: String,
    pub wasm_tools_version: String,
    archives: BTreeMap<String, String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl Pins {
    pub fn load(root: &Path) -> io::Result<Self> {
        Self::parse(&fs::read_to_string(
            root.join("examples/wasm/toolchain.toml"),
        )?)
    }

    fn parse(raw: &str) -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        for line in raw.lines() {
            let line = line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| invalid("expected a Wasm toolchain assignment"))?;
            let key = key.trim();
            let value = value
                .trim()
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or_else(|| invalid(format!("expected a quoted value for {key}")))?;
            if entries.insert(key, value).is_some() {
                return Err(invalid(format!("duplicate Wasm toolchain pin: {key}")));
            }
        }
        let mut take = |key: &str, valid: fn(&str) -> bool| -> io::Result<String> {
            let value = entries
                .remove(key)
                .ok_or_else(|| invalid(format!("missing Wasm toolchain pin: {key}")))?;
            if !valid(value) {
                return Err(invalid(format!("invalid Wasm toolchain pin: {key}")));
            }
            Ok(value.to_owned())
        };
        fn version(s: &str) -> bool {
            let mut parts = s.split('.');
            (0..3).all(|_| {
                parts
                    .next()
                    .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            }) && parts.next().is_none()
        }
        fn digest(s: &str, n: usize) -> bool {
            s.len() == n && s.bytes().all(|b| matches!(b,b'0'..=b'9'|b'a'..=b'f'))
        }
        fn name(s: &str) -> bool {
            !s.is_empty()
                && s.bytes().next().unwrap().is_ascii_alphanumeric()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        }
        let rust_version = take("rust_version", version)?;
        let rust_commit = take("rust_commit", |s| digest(s, 40))?;
        let rust_manifest_sha256 = take("rust_manifest_sha256", |s| digest(s, 64))?;
        let guest_target = take("guest_target", name)?;
        let ledger_guest_target = take("ledger_guest_target", name)?;
        let guest_profile = take("guest_profile", name)?;
        let wasm_tools_version = take("wasm_tools_version", version)?;
        let mut archives = BTreeMap::new();
        for key in [
            "aarch64_macos",
            "x86_64_macos",
            "aarch64_linux",
            "x86_64_linux",
        ] {
            archives.insert(key.into(), take(key, |s| digest(s, 64))?);
        }
        if let Some(k) = entries.keys().next() {
            return Err(invalid(format!("unknown Wasm toolchain pin: {k}")));
        }
        Ok(Self {
            rust_version,
            rust_commit,
            rust_manifest_sha256,
            guest_target,
            ledger_guest_target,
            guest_profile,
            wasm_tools_version,
            archives,
        })
    }
    pub fn archive(&self, arch: &str, os: &str) -> io::Result<(String, &str)> {
        let sha = self
            .archives
            .get(&format!("{arch}_{os}"))
            .ok_or_else(|| invalid(format!("unsupported wasm-tools host: {arch}-{os}")))?;
        Ok((
            format!("wasm-tools-{}-{arch}-{os}", self.wasm_tools_version),
            sha,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PINS: &str = include_str!("../examples/wasm/toolchain.toml");
    #[test]
    fn bind_host_and_reject_malformed_pins() {
        let p = Pins::parse(PINS).unwrap();
        for arch in ["aarch64", "x86_64"] {
            for os in ["macos", "linux"] {
                assert_eq!(p.archive(arch, os).unwrap().1.len(), 64);
            }
        }
        assert!(p.archive("aarch64", "windows").is_err());
        assert!(Pins::parse(&(PINS.to_owned() + "\nrust_version=\"1.0.0\"")).is_err());
        assert!(Pins::parse(&PINS.replace(&p.rust_commit, "invalid")).is_err());
        assert!(Pins::parse(&PINS.replace(&p.rust_manifest_sha256, "00")).is_err());
        assert!(Pins::parse(&PINS.replace(&p.wasm_tools_version, "1.2.3/../other")).is_err());
    }
}
