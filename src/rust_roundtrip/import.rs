use super::{FileSyntax, parser};
use crate::contracts::normal;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

type CapturedSources = BTreeMap<String, (Vec<u8>, FileSyntax)>;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    target_directory: PathBuf,
    build_directory: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    edition: String,
    manifest_path: PathBuf,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    src_path: PathBuf,
    edition: String,
}

pub(super) fn capture(
    manifest: &Path,
    package_name: &str,
) -> Result<(String, CapturedSources), String> {
    let manifest = manifest
        .canonicalize()
        .map_err(|error| format!("manifest {}: {error}", manifest.display()))?;
    let current_dir = manifest
        .parent()
        .ok_or("manifest has no parent directory")?;
    let output = Command::new("cargo")
        .current_dir(current_dir)
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--no-deps",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .map_err(|error| format!("run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata: {error}"))?;
    let mut matches = metadata
        .packages
        .into_iter()
        .filter(|package| package.name == package_name);
    let package = matches
        .next()
        .ok_or_else(|| format!("Cargo workspace has no package named {package_name:?}"))?;
    if matches.next().is_some() {
        return Err(format!("ambiguous Cargo package name {package_name:?}"));
    }
    let root = package
        .manifest_path
        .parent()
        .ok_or("package manifest has no parent directory")?
        .canonicalize()
        .map_err(|error| format!("package directory: {error}"))?;
    let paths = rust_paths(
        &root,
        &metadata.target_directory,
        metadata.build_directory.as_deref(),
    )?;
    for target in &package.targets {
        if target.edition != package.edition {
            return Err(format!(
                "Cargo target uses a different edition: {}",
                target.src_path.display()
            ));
        }
        let path = target
            .src_path
            .canonicalize()
            .map_err(|error| format!("Cargo target {}: {error}", target.src_path.display()))?;
        let relative = rust_path(&root, &path)?;
        if !paths.contains_key(relative) {
            return Err(format!(
                "Cargo target is outside the selected Rust inputs: {relative}"
            ));
        }
    }
    if paths.is_empty() {
        return Err(format!(
            "Cargo package {package_name:?} has no Rust sources"
        ));
    }

    let files = paths
        .into_iter()
        .map(|(relative, path)| {
            let bytes = fs::read(path).map_err(|error| format!("{relative}: {error}"))?;
            let text =
                std::str::from_utf8(&bytes).map_err(|error| format!("{relative}: {error}"))?;
            let tree = parser::parse(text, &package.edition)
                .map_err(|error| format!("{relative}: {error}"))?;
            Ok((relative, (bytes, tree)))
        })
        .collect::<Result<_, String>>()?;
    Ok((package.edition, files))
}

fn rust_paths(
    root: &Path,
    target_dir: &Path,
    build_dir: Option<&Path>,
) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut directories = vec![root.to_owned()];
    let mut paths = BTreeMap::new();
    while let Some(directory) = directories.pop() {
        let mut entries = fs::read_dir(&directory)
            .and_then(|entries| entries.collect::<Result<Vec<_>, _>>())
            .map_err(|error| format!("read {}: {error}", directory.display()))?;
        entries.sort_unstable_by_key(|entry| entry.file_name());
        if directory != root
            && entries
                .iter()
                .any(|entry| entry.file_name() == "Cargo.toml")
        {
            // A manifest below the selected package establishes a separate capture root.
            continue;
        }
        for entry in entries {
            let path = entry.path();
            if entry.file_name() == ".git" || path == target_dir || build_dir == Some(&path) {
                continue;
            }
            let kind = entry
                .file_type()
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if kind.is_symlink() {
                let relative = path
                    .strip_prefix(root)
                    .expect("walk starts at package root");
                return Err(format!("symlink in Cargo package: {}", relative.display()));
            }
            if kind.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = rust_path(root, &path)?;
                if !kind.is_file() {
                    return Err(format!("Rust input is not a regular file: {relative}"));
                }
                paths.insert(relative.to_owned(), path);
            }
        }
    }
    Ok(paths)
}

fn rust_path<'a>(root: &Path, path: &'a Path) -> Result<&'a str, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| {
            format!(
                "Rust input is outside the Cargo package: {}",
                path.display()
            )
        })?
        .to_str()
        .ok_or("Rust source path is not UTF-8")?;
    if !normal(relative) || !relative.ends_with(".rs") {
        return Err(format!("unsupported Rust source path {relative:?}"));
    }
    Ok(relative)
}

#[cfg(test)]
mod tests {
    use super::capture;
    use std::{fs, path::Path};

    fn prepare_package(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"source-capture-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub const VALUE: u32 = 18;\n").unwrap();
    }

    #[test]
    fn captures_package_rust_without_nested_packages_or_target_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        prepare_package(root);
        fs::create_dir_all(root.join("src/inactive")).unwrap();
        fs::write(
            root.join("src/inactive/extra.rs"),
            b"const EXTRA: u8 = 2;\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("target/out")).unwrap();
        fs::write(
            root.join("target/out/old.rs"),
            b"invalid as standalone Rust",
        )
        .unwrap();
        prepare_package(&root.join("embedded"));
        let (edition, files) = capture(&root.join("Cargo.toml"), "source-capture-test").unwrap();
        assert_eq!(edition, "2024");
        assert_eq!(
            files.keys().map(String::as_str).collect::<Vec<_>>(),
            ["src/inactive/extra.rs", "src/lib.rs"]
        );
        assert_eq!(files["src/inactive/extra.rs"].0, b"const EXTRA: u8 = 2;\n");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_to_rust_outside_the_package() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("package");
        prepare_package(&root);
        let external = temp.path().join("external.rs");
        fs::write(&external, b"const EXTERNAL: u8 = 1;\n").unwrap();
        std::os::unix::fs::symlink(external, root.join("src/external.rs")).unwrap();
        let error = capture(&root.join("Cargo.toml"), "source-capture-test").unwrap_err();
        assert!(
            error.contains("symlink") && error.contains("src/external.rs"),
            "{error}"
        );
    }
}
