//! Portable contracts for bounded tagged binary records.
mod graph;
mod rust;
mod typescript;
mod wire;

pub use graph::load;
pub use wire::{Contract, Decoder, Feed, Value};

use std::{fs, path::Path};

pub fn recover(
    source: &Path,
    selection: &Path,
    overlay: &Path,
    variant: &str,
    out: &Path,
) -> Result<(), String> {
    use crate::rust_recovery::tagged;
    let selection = fs::read(selection).map_err(|err| format!("read selection: {err}"))?;
    let selection =
        serde_json::from_slice(&selection).map_err(|err| format!("Rust selection: {err}"))?;
    let candidate = tagged::recover(source, &selection)?;
    let complete = graph::complete_tag(
        overlay,
        variant,
        &selection.variant,
        candidate.encoded_tag.value,
        &candidate.fields,
    )?;
    use std::io::Write;
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| format!("stage wire graph: {err}"))?;
    staged
        .write_all(&complete)
        .map_err(|err| format!("write wire graph: {err}"))?;
    staged
        .persist_noclobber(out)
        .map_err(|err| format!("publish wire graph: {err}"))?;
    Ok(())
}

pub fn build(graph: &Path, pin: &str, target: &str, output: &Path) -> Result<(), String> {
    let contract = load(graph)?;
    if contract.sha256() != pin {
        return Err("tagged wire graph digest mismatch".into());
    }
    let (file, source) = match target {
        "rust" => ("wire.rs", rust::render(&contract)),
        "typescript" => ("wire.mts", typescript::render(&contract)?),
        _ => return Err(format!("unsupported tagged wire target {target}")),
    };
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage =
        tempfile::tempdir_in(parent).map_err(|err| format!("stage tagged wire output: {err}"))?;
    let lock = serde_json::json!({
        "vocabulary": "oddlang-tagged-wire-v1",
        "graphSha256": pin,
        "target": target,
        "source": file,
    });
    fs::write(stage.path().join("projection.lock"), format!("{lock:#}\n"))
        .and_then(|()| fs::write(stage.path().join(file), source))
        .map_err(|err| format!("write tagged wire output: {err}"))?;
    fs::rename(stage.path(), output).map_err(|err| format!("publish tagged wire output: {err}"))
}
