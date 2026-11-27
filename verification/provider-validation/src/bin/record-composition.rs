//! Compare a legacy recovery, standalone projection and composed package across compilers.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const SOURCE: &str = "#[derive(Clone, Debug, Default)]\npub struct Contour {\n    pub label: String,\n    pub cells: usize,\n}\n";
const ARTIFACTS: &[&str] = &[
    "model/graph.json",
    "model/rust.json",
    "model/recovery.lock",
    "projection/records.rs",
    "projection/projection.lock",
    "product/Cargo.toml",
    "product/Cargo.lock",
    "product/package.lock",
    "product/src/lib.rs",
    "product/src/group_0/mod.rs",
    "product/src/group_0/__projection_0.rs",
];

fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn write(path: &Path, value: Value) -> Result<()> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&value)?))?;
    Ok(())
}

fn invoke(cli: &Path, root: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new(cli).args(args).current_dir(root).status()?;
    if !status.success() {
        return Err(format!("{} {args:?}: {status}", cli.display()).into());
    }
    Ok(())
}

fn artifacts(cli: &Path, root: &Path) -> Result<BTreeMap<&'static str, Vec<u8>>> {
    fs::create_dir(root)?;
    fs::write(root.join("source.rs"), SOURCE)?;
    write(
        &root.join("selection.json"),
        json!({"format":"oddlang-rust-root-record-selection-v1",
        "sourceSha256":sha(SOURCE.as_bytes()),"id":"urn:terrain:root","records":{"Contour":"urn:terrain:contour"}}),
    )?;
    invoke(
        cli,
        root,
        &[
            "recover",
            "public-root-records",
            "source.rs",
            "selection.json",
            "model",
        ],
    )?;
    invoke(
        cli,
        root,
        &[
            "build",
            "public-records",
            "rust",
            "model/graph.json",
            "model/rust.json",
            "projection",
        ],
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"terrain-fixture\"\nversion = \"0.0.0\"\n",
    )?;
    let pin = |path: &str| -> Result<Value> {
        Ok(json!({"path":path,"sha256":sha(&fs::read(root.join(path))?)}))
    };
    write(
        &root.join("package.json"),
        json!({"format":"oddlang-native-package-v1","name":"terrain-fixture","version":"0.0.0",
        "edition":"2024","cargoLock":pin("Cargo.lock")?,"components":[{"kind":"public-records","module":"contours",
        "reexports":["Contour"],"graph":pin("model/graph.json")?,"overlay":pin("model/rust.json")?}]}),
    )?;
    invoke(
        cli,
        root,
        &["build", "native-package", "rust", "package.json", "product"],
    )?;
    ARTIFACTS
        .iter()
        .map(|&path| Ok((path, fs::read(root.join(path))?)))
        .collect()
}

fn main() -> Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let [old, new] = args.as_slice() else {
        return Err("usage: record-composition OLD_CLI NEW_CLI".into());
    };
    let temp = tempfile::tempdir()?;
    let before = artifacts(&old.canonicalize()?, &temp.path().join("before"))?;
    let after = artifacts(&new.canonicalize()?, &temp.path().join("after"))?;
    let mut digests = BTreeMap::new();
    for (&path, data) in &after {
        if before[path] != *data {
            return Err(format!("legacy output changed: {path}").into());
        }
        digests.insert(path, sha(data));
    }
    let receipt = json!({"oldCompilerSha256":sha(&fs::read(old)?),"newCompilerSha256":sha(&fs::read(new)?),"artifacts":digests});
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
