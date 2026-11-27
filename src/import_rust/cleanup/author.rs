//! Freeze a crate or propose and accept a specific source/mapping patch.
use super::{Receipt, Res, digest, inventory, read, tree_digest};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
fn io<T>(value: std::io::Result<T>) -> Res<T> {
    value.map_err(|e| e.to_string())
}

fn new_path(path: &Path) -> Res<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let resolved =
        io(parent.canonicalize())?.join(path.file_name().ok_or("expected output filename")?);
    match resolved.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(resolved),
        Err(e) => Err(e.to_string()),
        Ok(_) => Err("output already exists".into()),
    }
}

fn create_file(path: &Path, bytes: &[u8]) -> Res<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = io(tempfile::NamedTempFile::new_in(parent))?;
    io(file.write_all(bytes))?;
    file.persist_noclobber(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn freeze(root: &Path, pin: &Path) -> Res<()> {
    let output = new_path(pin)?;
    if output.starts_with(io(root.canonicalize())?) {
        return Err("write independent baseline pin outside input".into());
    }
    let files = inventory(root)?;
    if files.is_empty() {
        return Err("empty baseline".into());
    }
    create_file(
        &output,
        format!("oddlang-rust-cleanup-baseline-v1 {}\n", tree_digest(&files)).as_bytes(),
    )
}

fn capture(source: &Path, dest: &Path) -> Res<()> {
    let files = inventory(source)?;
    io(fs::create_dir(dest))?;
    for (name, expected) in files {
        let path = dest.join(&name);
        io(fs::create_dir_all(path.parent().unwrap()))?;
        let bytes = read(&source.join(&name))?;
        if digest(&bytes) != expected {
            return Err(format!("input changed during capture: {name}"));
        }
        io(fs::write(path, bytes))?;
    }
    Ok(())
}

fn receipt(root: &Path, manifest: &str, mapping: &str) -> Res<Receipt> {
    let before = inventory(&root.join("original"))?;
    let after = inventory(&root.join("effective"))?;
    if !before.contains_key(manifest)
        || !before.contains_key(mapping)
        || before.keys().ne(after.keys())
    {
        return Err("cleanup requires existing files and the pinned manifest and mapping".into());
    }
    if before == after
        || before
            .iter()
            .any(|(name, hash)| hash != &after[name] && !(name.ends_with(".rs") || name == mapping))
    {
        return Err("expected changes to Rust source or the pinned mapping".into());
    }
    Ok(Receipt {
        version: 1,
        manifest: manifest.into(),
        mapping: mapping.into(),
        original_tree_sha256: tree_digest(&before),
        effective_tree_sha256: tree_digest(&after),
        patch_sha256: digest(&read(&root.join("cleanup.patch"))?),
    })
}

// Historical proposals use sorted JSON keys and ASCII escapes in the reviewed digest.
fn encode(record: &Receipt) -> Res<Vec<u8>> {
    let sorted = serde_json::to_value(record).map_err(|e| e.to_string())?;
    let pretty = serde_json::to_string_pretty(&sorted).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    for c in pretty.chars() {
        if c.is_ascii() && c != '\u{7f}' {
            bytes.push(c as u8);
        } else {
            for unit in c.encode_utf16(&mut [0u16; 2]) {
                write!(bytes, "\\u{unit:04x}").unwrap();
            }
        }
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn propose(old: &Path, new: &Path, out: &Path, manifest: &str, mapping: &str) -> Res<()> {
    let dest = new_path(out)?;
    for src in [old, new] {
        if dest.starts_with(io(src.canonicalize())?) {
            return Err("choose an output outside both inputs".into());
        }
    }
    let staging = io(tempfile::tempdir_in(dest.parent().unwrap()))?;
    let root = staging.path().join("proposal");
    io(fs::create_dir(&root))?;
    capture(old, &root.join("original"))?;
    capture(new, &root.join("effective"))?;
    let diff = io(Command::new("git")
        .current_dir(&root)
        .args([
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--binary",
            "--",
            "original",
            "effective",
        ])
        .output())?;
    if diff.status.code() != Some(1) {
        return Err("expected a nonempty diff".into());
    }
    io(fs::write(root.join("cleanup.patch"), diff.stdout))?;
    let bytes = encode(&receipt(&root, manifest, mapping)?)?;
    io(fs::write(root.join("proposal.json"), &bytes))?;
    crate::native_package::publish(&root, &dest)?;
    println!("{}", digest(&bytes));
    Ok(())
}

fn accept(root: &Path, pin: &str) -> Res<()> {
    let bytes = read(&root.join("proposal.json"))?;
    if digest(&bytes) != pin {
        return Err("review digest mismatch".into());
    }
    let record: Receipt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if receipt(root, &record.manifest, &record.mapping)? != record {
        return Err("proposal changed after review".into());
    }
    create_file(&root.join("accepted.json"), &bytes)
}

pub fn command(args: &[String]) -> Res<()> {
    match args {
        [cmd,old,pin] if cmd=="freeze" => freeze(old.as_ref(),pin.as_ref()),
        [cmd,root,flag,pin] if cmd=="accept" && flag=="--sha256" => accept(root.as_ref(),pin),
        [cmd,old,new,out,options @ ..] if cmd=="propose" => {
            let (mut manifest,mut mapping)=("Cargo.toml","mapping.toml");
            for pair in options.chunks(2) {
                match pair {
                    [flag,file] if flag=="--manifest" => manifest=file,
                    [flag,file] if flag=="--mapping" => mapping=file,
                    _ => return Err("expected --manifest FILE or --mapping FILE".into()),
                }
            }
            propose(old.as_ref(),new.as_ref(),out.as_ref(),manifest,mapping)
        }
        _=>Err("usage: oddlang cleanup freeze ORIGINAL PIN | propose ORIGINAL EDITED OUTPUT [--manifest FILE] [--mapping FILE] | accept PROPOSAL --sha256 DIGEST".into())
    }
}
