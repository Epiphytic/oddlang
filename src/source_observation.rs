//! Source and output checks shared by compiler-backed recovery profiles.
use crate::wire_schema::digest;
use std::{collections::BTreeMap, fs, io::Write as _, path::Path};

type Result<T> = std::result::Result<T, String>;
pub(crate) const COMPILER: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))
}

fn canonical(path: &Path) -> Result<std::path::PathBuf> {
    path.canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", path.display()))
}

pub(crate) fn validate_source_files(
    root: &Path,
    source: &Path,
    file: Option<&str>,
    sha: Option<&str>,
    files: &BTreeMap<String, String>,
) -> Result<()> {
    let root = canonical(root)?;
    let source = canonical(source)?;
    let selected = source.strip_prefix(&root).ok().and_then(Path::to_str);
    if selected != file
        || file.and_then(|file| files.get(file).map(String::as_str)) != sha
        || files.is_empty()
        || files.len() > 4096
    {
        return Err("compiler source inventory does not contain the selected file".into());
    }
    for (name, hash) in files {
        let relative = Path::new(name);
        if relative.as_os_str().is_empty()
            || !relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(format!("invalid compiler source path: {name}"));
        }
        let path = canonical(&root.join(relative))?;
        if !path.starts_with(&root) || digest(&read(&path)?) != *hash {
            return Err(format!("compiler source inventory changed: {name}"));
        }
    }
    Ok(())
}

pub(crate) fn publish(output: &Path, encoded: &[u8]) -> Result<()> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|err| format!("cannot create output directory: {err}"))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| format!("cannot stage recovered observation: {err}"))?;
    staged
        .write_all(encoded)
        .map_err(|err| format!("cannot stage recovered observation: {err}"))?;
    staged
        .persist_noclobber(output)
        .map(|_| ())
        .map_err(|err| format!("cannot publish {}: {err}", output.display()))
}
