//! Publish a source workspace from a pinned, closed input store.
use crate::contracts::{normal, verified};
use proc_macro2::{TokenStream, TokenTree};
use serde::{Deserialize, de::MapAccess};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Project {
    version: u32,
    crates: Vec<String>,
    target: String,
    features: Vec<String>,
    graph_sha256: String,
    projection_sha256: String,
    cargo_lock_sha256: String,
    files: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    mode: String,
    producer: Producer,
    sha256: String,
    module: Option<String>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Producer {
    Generated,
    Retained,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Projection {
    version: u32,
    graph_sha256: String,
    #[serde(deserialize_with = "unique_modules")]
    files: BTreeMap<String, String>,
}

fn unique_modules<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Modules;
    impl<'de> serde::de::Visitor<'de> for Modules {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map of unique emitted modules")
        }
        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut files = BTreeMap::new();
            while let Some((name, hash)) = map.next_entry::<String, String>()? {
                if files.insert(name.clone(), hash).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate emitted module {name}"
                    )));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(Modules)
}

fn digest(value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        Ok(())
    } else {
        Err("expected a lowercase SHA-256 digest".into())
    }
}

fn portable(name: &str) -> Result<()> {
    if !normal(name)
        || name.split('/').any(|part| {
            part.ends_with(['.', ' '])
                || part.eq_ignore_ascii_case(".git")
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-+@ ".contains(&c))
        })
    {
        return Err(format!("invalid portable workspace path {name:?}"));
    }
    Ok(())
}

fn file_type(path: &Path) -> Result<fs::FileType> {
    fs::symlink_metadata(path)
        .map(|s| s.file_type())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn regular_bytes(path: &Path) -> Result<Vec<u8>> {
    if !file_type(path)?.is_file() {
        return Err(format!("expected regular file {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn verified_file(root: &Path, path: &Path, hash: &str) -> Result<Vec<u8>> {
    if !file_type(&root.join(path))?.is_file() {
        return Err(format!("expected regular store file {}", path.display()));
    }
    verified(root, path.to_str().ok_or("invalid store name")?, hash)
}

fn directory(path: &Path) -> Result<()> {
    if !file_type(path)?.is_dir() {
        return Err(format!("expected directory {}", path.display()));
    }
    Ok(())
}

// Track every prefix as well as each leaf. This also rejects case aliases on
// filesystems where different spellings resolve to the same directory.
fn register_path(paths: &mut BTreeMap<String, (String, bool)>, name: &str) -> Result<()> {
    portable(name)?;
    let mut prefix = String::new();
    for part in name.split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        let leaf = prefix == name;
        let key = prefix.to_ascii_lowercase();
        if let Some((prior, is_file)) = paths.get(&key) {
            if *is_file || leaf || *prior != prefix {
                return Err(format!("workspace path collision at {prefix}"));
            }
        } else {
            paths.insert(key, (prefix.clone(), leaf));
        }
    }
    Ok(())
}

fn walk(root: &Path, dir: &Path, entries: &mut BTreeSet<PathBuf>) -> Result<()> {
    let mut children = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        let path = entry.path();
        let kind = file_type(&path)?;
        entries.insert(path.strip_prefix(root).unwrap().to_owned());
        if kind.is_dir() {
            walk(root, &path, entries)?;
        } else if !kind.is_file() {
            return Err(format!("forbidden store entry {}", path.display()));
        }
    }
    Ok(())
}

fn check_store(root: &Path, inputs: &[PathBuf]) -> Result<()> {
    let mut expected = BTreeSet::new();
    for file in inputs {
        expected.extend(
            file.ancestors()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_owned),
        );
    }
    let mut actual = BTreeSet::new();
    walk(root, root, &mut actual)?;
    if let Some(difference) = actual.symmetric_difference(&expected).next() {
        return Err(format!(
            "missing or undeclared store entry {}",
            difference.display()
        ));
    }
    Ok(())
}

fn local_reference(root: &Path, owner: &Path, value: &str, file: bool) -> Result<()> {
    let relative = Path::new(value);
    if value.is_empty() || value.contains('\\') || relative.is_absolute() {
        return Err(format!(
            "invalid local input in {}: {value}",
            owner.display()
        ));
    }
    let mut resolved = owner.parent().unwrap().to_path_buf();
    for part in relative.components() {
        match part {
            Component::Normal(name) => resolved.push(name),
            Component::CurDir => {}
            Component::ParentDir if resolved.pop() => {}
            _ => return Err(format!("input escapes workspace: {value}")),
        }
    }
    let path = root.join(resolved);
    if (file && !path.is_file()) || !path.exists() {
        return Err(format!(
            "undeclared local input in {}: {value}",
            owner.display()
        ));
    }
    Ok(())
}

fn module_attribute(root: &Path, owner: &Path, generated: bool, stream: TokenStream) -> Result<()> {
    let mut tokens = stream.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Ident(ident)
                if ident == "path"
                    && matches!(tokens.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '=') =>
            {
                if generated {
                    return Err(format!(
                        "emitted module {} redirects a module path",
                        owner.display()
                    ));
                }
                tokens.next();
                let literal = tokens.next().ok_or("missing Rust module path")?;
                let path = syn::parse2::<syn::LitStr>(TokenStream::from(literal))
                    .map_err(|_| format!("dynamic Rust module path in {}", owner.display()))?;
                local_reference(root, owner, &path.value(), true)?;
            }
            TokenTree::Group(group) => module_attribute(root, owner, generated, group.stream())?,
            _ => {}
        }
    }
    Ok(())
}

fn rust_includes(root: &Path, owner: &Path, generated: bool, stream: TokenStream) -> Result<()> {
    let tokens: Vec<_> = stream.into_iter().collect();
    for (i, token) in tokens.iter().enumerate() {
        if let TokenTree::Group(group) = token {
            if i > 0
                && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '#')
                && group.delimiter() == proc_macro2::Delimiter::Bracket
                && matches!(group.stream().into_iter().next(), Some(TokenTree::Ident(name)) if name == "path" || name == "cfg_attr")
            {
                module_attribute(root, owner, generated, group.stream())?;
            }
            rust_includes(root, owner, generated, group.stream())?;
        }
        let TokenTree::Ident(name) = token else {
            continue;
        };
        if !matches!(
            name.to_string().as_str(),
            "include" | "include_str" | "include_bytes"
        ) || !matches!(tokens.get(i+1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
        {
            continue;
        }
        if generated {
            return Err(format!(
                "emitted module {} includes source or data",
                owner.display()
            ));
        }
        let Some(TokenTree::Group(args)) = tokens.get(i + 2) else {
            return Err("invalid Rust include".into());
        };
        let value = syn::parse2::<syn::LitStr>(args.stream())
            .map_err(|_| format!("dynamic Rust include in {}", owner.display()))?;
        local_reference(root, owner, &value.value(), true)?;
    }
    Ok(())
}

fn cargo_paths(root: &Path, owner: &Path, value: &toml::Value) -> Result<()> {
    match value {
        toml::Value::Array(items) => {
            for item in items {
                cargo_paths(root, owner, item)?;
            }
        }
        toml::Value::Table(table) => {
            for (key, item) in table {
                if key == "metadata" {
                    continue;
                }
                if key == "path"
                    && let Some(path) = item.as_str()
                {
                    local_reference(root, owner, path, false)?;
                }
                cargo_paths(root, owner, item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn checked_sources(project: &Project, root: &Path) -> Result<()> {
    let mut packages = BTreeMap::new();
    for row in &project.files {
        let owner = Path::new(&row.path);
        let path = root.join(owner);
        if owner.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
            let tokens = text
                .parse()
                .map_err(|_| format!("invalid Rust tokens in {}", row.path))?;
            rust_includes(root, owner, row.producer == Producer::Generated, tokens)?;
        } else if owner.file_name().is_some_and(|s| s == "Cargo.toml") {
            let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
            let config: toml::Value =
                toml::from_str(&text).map_err(|e| format!("{}: {e}", row.path))?;
            cargo_paths(root, owner, &config)?;
            if let Some(workspace) = config.get("workspace") {
                for key in ["members", "default-members", "exclude"] {
                    for member in workspace
                        .get(key)
                        .and_then(toml::Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let value = member.as_str().ok_or("expected Cargo workspace path")?;
                        // Globs are expanded by Cargo in the independently verified build.
                        // Check their anchor even when an exclude pattern selects no files.
                        let anchor = value
                            .split('/')
                            .take_while(|part| !part.contains(['*', '?', '[']))
                            .collect::<Vec<_>>();
                        if value.starts_with('/')
                            || value.contains('\\')
                            || value.split('/').any(|part| part == "..")
                        {
                            return Err(format!("invalid Cargo workspace selector {value}"));
                        }
                        if key != "exclude" && !anchor.is_empty() {
                            local_reference(root, owner, &anchor.join("/"), false)?;
                        }
                    }
                }
            }
            if let Some(package) = config.get("package") {
                if let Some(workspace) = package.get("workspace").and_then(toml::Value::as_str) {
                    local_reference(root, owner, workspace, false)?;
                }
                if let Some(build) = package.get("build").and_then(toml::Value::as_str) {
                    local_reference(root, owner, build, true)?;
                }
                if let Some(name) = package.get("name").and_then(toml::Value::as_str)
                    && project.crates.iter().any(|c| c == name)
                {
                    let features = config
                        .get("features")
                        .and_then(toml::Value::as_table)
                        .into_iter()
                        .flat_map(|t| t.keys().cloned())
                        .collect::<BTreeSet<_>>();
                    if packages.insert(name.to_owned(), features).is_some() {
                        return Err(format!("ambiguous package {name}"));
                    }
                }
            }
        }
    }
    for name in &project.crates {
        if !packages.contains_key(name) {
            return Err(format!("missing selected Cargo package {name}"));
        }
    }
    for feature in &project.features {
        let (name, selected) = feature
            .split_once('/')
            .ok_or_else(|| format!("unqualified feature {feature}"))?;
        if !packages
            .get(name)
            .is_some_and(|features| features.contains(selected))
        {
            return Err(format!("unknown selected feature {feature}"));
        }
    }
    Ok(())
}

pub fn assemble(manifest: &Path, store: &Path, dest: &Path) -> Result<()> {
    let manifest_bytes = regular_bytes(manifest)?;
    let project: Project =
        toml::from_str(std::str::from_utf8(&manifest_bytes).map_err(|e| e.to_string())?)
            .map_err(|e| format!("assembly manifest: {e}"))?;
    if project.version != 1
        || project.crates.is_empty()
        || project.target.is_empty()
        || !project
            .target
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        || project.crates.iter().collect::<BTreeSet<_>>().len() != project.crates.len()
        || project.features.iter().collect::<BTreeSet<_>>().len() != project.features.len()
    {
        return Err("invalid assembly version or build selection".into());
    }
    for hash in [
        &project.graph_sha256,
        &project.projection_sha256,
        &project.cargo_lock_sha256,
    ] {
        digest(hash)?;
    }

    directory(store)?;
    let store = store.canonicalize().map_err(|e| e.to_string())?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if parent.starts_with(&store) {
        return Err("destination inside input store".into());
    }
    let dest = parent.join(dest.file_name().ok_or("invalid destination")?);
    match fs::symlink_metadata(&dest) {
        Ok(meta)
            if meta.is_dir()
                && fs::read_dir(&dest)
                    .map_err(|e| e.to_string())?
                    .next()
                    .is_none() => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err("destination must be absent or an empty directory".into()),
    }

    let projection_dir = Path::new("projections").join(&project.projection_sha256);
    directory(&store.join("projections"))?;
    directory(&store.join(&projection_dir))?;
    let receipt = projection_dir.join("projection.json");
    let projection: Projection = serde_json::from_slice(&verified_file(
        &store,
        &receipt,
        &project.projection_sha256,
    )?)
    .map_err(|e| format!("projection receipt: {e}"))?;
    if projection.version != 1
        || projection.graph_sha256 != project.graph_sha256
        || projection.files.is_empty()
    {
        return Err("projection disagrees with the pinned graph or has no modules".into());
    }

    let mut projection_paths = BTreeMap::new();
    for name in projection.files.keys() {
        register_path(&mut projection_paths, name)?;
    }

    let mut paths = BTreeMap::new();
    let mut modules = BTreeSet::new();
    let mut inputs = vec![receipt];
    let mut root_manifest = false;
    let mut locked = false;
    for row in &project.files {
        register_path(&mut paths, &row.path)?;
        digest(&row.sha256)?;
        if !matches!(row.mode.as_str(), "100644" | "100755") {
            return Err(format!("unsupported mode for {}", row.path));
        }
        let source = match (&row.producer, &row.module) {
            (Producer::Retained, None) => Path::new("retained").join(&row.sha256),
            (Producer::Generated, Some(name)) => {
                portable(name)?;
                if !name.ends_with(".rs")
                    || !row.path.ends_with(".rs")
                    || row.mode != "100644"
                    || !modules.insert(name)
                    || projection.files.get(name) != Some(&row.sha256)
                {
                    return Err(format!("unverified or reused emitted module {name}"));
                }
                projection_dir.join(name)
            }
            _ => {
                return Err(format!(
                    "producer disagrees with module mapping for {}",
                    row.path
                ));
            }
        };
        root_manifest |= row.path == "Cargo.toml";
        locked |= row.path == "Cargo.lock" && row.sha256 == project.cargo_lock_sha256;
        inputs.push(source);
    }
    if !locked || !root_manifest || modules.len() != projection.files.len() {
        return Err("missing root Cargo manifest/lock or unconsumed projection module".into());
    }
    check_store(&store, &inputs)?;
    let stage = tempfile::Builder::new()
        .prefix(".oddlang-assembly-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    for (row, input) in project.files.iter().zip(&inputs[1..]) {
        let bytes = verified_file(&store, input, &row.sha256)?;
        let out = stage.path().join(&row.path);
        fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&out, bytes).map_err(|e| e.to_string())?;
        let mode = if row.mode == "100755" { 0o755 } else { 0o644 };
        fs::set_permissions(out, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    }
    checked_sources(&project, stage.path())?;
    fs::rename(stage.path(), dest).map_err(|e| format!("publish assembled workspace: {e}"))
}
