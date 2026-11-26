#![cfg(test)]
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
mod catalog;

#[derive(Debug)]
enum Fault {
    Io(std::io::Error),
    Invalid(&'static str),
}

mod formats {
    use super::*;
    pub struct Asset {
        pub name: &'static str,
        pub length: u64,
        pub mode: u32,
        pub digest: [u8; 32],
    }
    pub struct Catalog {
        pub family: &'static str,
        pub items: Vec<Asset>,
    }
    impl Catalog {
        pub fn parse(input: &[u8]) -> Result<Self, Fault> {
            let family = match input {
                b"family 1\n" => "v1",
                b"family 2\n" => "v2",
                _ => return Err(Fault::Invalid("syntax")),
            };
            Ok(Self {
                family,
                items: ["record", "extra"]
                    .map(|name| Asset {
                        name,
                        length: 1,
                        mode: 0o600,
                        digest: Sha256::digest(b"x").into(),
                    })
                    .into(),
            })
        }
        pub fn label(&self, name: &str) -> String {
            format!("{}-report.{name}", self.family)
        }
    }
}

// Independent stand-ins for the declared fixture file-effect interface.
mod reads {
    use super::*;
    pub fn bounded(path: &Path, max: u64) -> Result<Vec<u8>, Fault> {
        let data = fs::read(path).map_err(Fault::Io)?;
        if data.len() as u64 > max {
            return Err(Fault::Invalid("overflow"));
        }
        Ok(data)
    }
    pub fn verify(
        path: &Path,
        size: u64,
        mode: u32,
        pin: &[u8; 32],
        fixed: bool,
    ) -> Result<(), Fault> {
        assert_eq!(mode, 0o600);
        assert_eq!(
            fixed,
            path.file_name()
                .unwrap()
                .as_encoded_bytes()
                .iter()
                .all(|&b| b != b'.')
        );
        let data = bounded(path, size)?;
        if data.len() as u64 != size || &<[u8; 32]>::from(Sha256::digest(&data)) != pin {
            return Err(Fault::Invalid("asset"));
        }
        Ok(())
    }
}

#[test]
fn catalog_discovery_and_guard_order() {
    use catalog::{ITEM_NAMES, Naming, inspect_catalog};
    let root = std::env::temp_dir().join(format!("inert-catalog-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let missing = root.join("absent");
    assert!(
        matches!(inspect_catalog(&missing, Naming::Selected, [0; 32]),
        Err(Fault::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
    let data = b"family 1\n";
    let pin = Sha256::digest(data).into();
    let good = root.join("v1-report.record.idx");
    fs::write(&good, data).unwrap();
    fs::write(root.join("v1-report.record"), b"x").unwrap();
    fs::write(root.join("v1-report.extra"), b"x").unwrap();
    assert_eq!(
        inspect_catalog(&root, Naming::Selected, pin)
            .unwrap()
            .family,
        "v1"
    );
    fs::remove_file(root.join("v1-report.extra")).unwrap();
    fs::write(root.join("v1-report.record"), b"y").unwrap();
    // The first asset fails before the missing second file can change the error.
    assert!(matches!(
        inspect_catalog(&root, Naming::Selected, pin),
        Err(Fault::Invalid("asset"))
    ));
    fs::write(root.join("v1-report.record"), b"x").unwrap();
    assert!(matches!(inspect_catalog(&root, Naming::Selected, pin),
        Err(Fault::Io(error)) if error.kind() == std::io::ErrorKind::NotFound));
    fs::write(root.join("v1-report.extra"), b"x").unwrap();
    use std::os::unix::ffi::OsStrExt;
    let non_utf8 = root.join(std::ffi::OsStr::from_bytes(b"v1-report.\xff.idx"));
    if cfg!(target_os = "linux") {
        fs::write(&non_utf8, b"not a UTF-8 candidate").unwrap();
        inspect_catalog(&root, Naming::Selected, pin).unwrap();
        fs::remove_file(non_utf8).unwrap();
        println!("Observed UTF-8 candidate exclusion on Linux");
    } else {
        println!("Linux-only non-UTF-8 fixture omitted on this filesystem");
    }
    let decoy = root.join("v1-report.garbage.idx");
    fs::write(&decoy, b"malformed").unwrap();
    assert!(matches!(
        inspect_catalog(&root, Naming::Selected, [0; 32]),
        Err(Fault::Invalid("catalog_path"))
    ));
    fs::remove_file(&good).unwrap();
    assert!(matches!(
        inspect_catalog(&root, Naming::Selected, pin),
        Err(Fault::Invalid("checksum"))
    ));
    let decoy_pin = Sha256::digest(b"malformed").into();
    assert!(matches!(
        inspect_catalog(&root, Naming::Selected, decoy_pin),
        Err(Fault::Invalid("syntax"))
    ));
    fs::write(&decoy, data).unwrap();
    assert!(matches!(
        inspect_catalog(&root, Naming::Selected, pin),
        Err(Fault::Invalid("catalog_path"))
    ));
    let fixed = root.join("catalog.txt");
    for name in ITEM_NAMES {
        fs::write(root.join(name), b"x").unwrap();
    }
    fs::write(&fixed, b"family 2\n").unwrap();
    assert_eq!(
        inspect_catalog(&root, Naming::Fixed, Sha256::digest(b"family 2\n").into())
            .unwrap()
            .family,
        "v2"
    );
    fs::remove_dir_all(root).unwrap();
}
