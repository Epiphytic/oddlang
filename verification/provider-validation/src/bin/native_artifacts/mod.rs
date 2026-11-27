//! Compute a relative-path inventory for independently assembled fresh trees.
use provider_validation::Result;
use std::{collections::BTreeMap, fs, path::Path};
pub fn inventory(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                visit(root, &path, files)?;
            } else {
                files.insert(
                    path.strip_prefix(root)?
                        .to_str()
                        .ok_or("invalid relative path")?
                        .to_owned(),
                    fs::read(&path)?,
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}
