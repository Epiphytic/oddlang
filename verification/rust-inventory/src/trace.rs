use crate::{Result, sha, string};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub type Roots = Vec<(&'static str, PathBuf)>;
pub type Producers = BTreeMap<PathBuf, String>;

pub use rust_inventory::{commands, flag};

fn dep_inputs(file: &Path) -> Result<Vec<String>> {
    let text = fs::read_to_string(file)?.replace("\\\n", "");
    let (_, values) = text
        .lines()
        .next()
        .and_then(|s| s.split_once(": "))
        .ok_or("missing dep-info prerequisites")?;
    let mut paths = Vec::new();
    let mut word = String::new();
    let mut chars = values.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            word.push(chars.next().ok_or("incomplete dep-info escape")?);
        } else if c.is_whitespace() {
            if !word.is_empty() {
                paths.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        paths.push(word);
    }
    Ok(paths)
}

pub fn package_id(identity: &str, root: &Path) -> String {
    let mut prefix = String::from("path+file://");
    for &b in root.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            prefix.push(b as char);
        } else {
            prefix.push_str(&format!("%{b:02X}"));
        }
    }
    match identity.strip_prefix(&prefix) {
        Some(tail) if tail.starts_with('/') || tail.starts_with('#') => {
            format!("workspace:{}", tail.trim_start_matches('/'))
        }
        _ => identity.to_owned(),
    }
}

pub fn identity(path: &Path, roots: &Roots, producers: &Producers) -> Result<Option<[String; 2]>> {
    for (scope, root) in roots {
        let Ok(tail) = path.strip_prefix(root) else {
            continue;
        };
        let (class, name) = match *scope {
            "registry-cache" => (
                "registry",
                string(&tail.components().skip(1).collect::<PathBuf>())?,
            ),
            "target" => {
                let (dir, package) = producers
                    .iter()
                    .find(|(dir, _)| path.starts_with(dir))
                    .ok_or_else(|| {
                        format!(
                            "no recorded producer for generated input {}",
                            path.display()
                        )
                    })?;
                (
                    "build",
                    format!("{package}/{}", string(path.strip_prefix(dir)?)?),
                )
            }
            other => (other, string(tail)?),
        };
        return Ok(Some([class.into(), name]));
    }
    Ok(None)
}

fn depfiles(root: &Path, all: &mut BTreeSet<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            depfiles(&entry.path(), all)?;
        } else if entry.path().extension().is_some_and(|s| s == "d") {
            all.insert(entry.path());
        }
    }
    Ok(())
}

pub fn invocations(
    root: &Path,
    target: &Path,
    compiler: &str,
    roots: &Roots,
    producers: &Producers,
    log: &str,
) -> Result<Vec<Value>> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    let identify = |name: &str| -> Result<[String; 3]> {
        let path = root.join(name).canonicalize()?;
        let [scope, name] = identity(&path, roots, producers)?
            .ok_or_else(|| format!("undeclared nightly compiler input {}", path.display()))?;
        Ok([scope, name, sha(&path)?])
    };
    for words in commands(log)? {
        let Some(index) = words.iter().position(|w| w == compiler) else {
            continue;
        };
        let env: BTreeMap<_, _> = words[..index]
            .iter()
            .filter_map(|w| w.split_once('='))
            .collect();
        let args = &words[index + 1..];
        let required = |name: &str| {
            flag(args, name)
                .first()
                .copied()
                .ok_or_else(|| format!("missing rustc {name}"))
        };
        let name = required("--crate-name")?;
        let suffix = flag(args, "-C")
            .into_iter()
            .find_map(|s| s.strip_prefix("extra-filename="))
            .unwrap_or("");
        let depfile = Path::new(required("--out-dir")?).join(format!("{name}{suffix}.d"));
        if !seen.insert(depfile.clone()) {
            return Err(format!("reused compiler dep-info {}", depfile.display()).into());
        }
        let inputs = dep_inputs(&depfile)?;
        let source = args
            .iter()
            .find(|s| inputs.contains(s))
            .ok_or_else(|| format!("compiler crate root missing in {}", depfile.display()))?;
        let mut ids = inputs
            .iter()
            .map(|p| identify(p))
            .collect::<Result<Vec<_>>>()?;
        ids.sort();
        let mut cfg = flag(args, "--cfg");
        cfg.sort();
        let aliases: BTreeSet<_> = flag(args, "--extern")
            .into_iter()
            .map(|s| s.split('=').next().unwrap())
            .collect();
        rows.push(json!({
            "crate": name,
            "package": [
                env.get("CARGO_PKG_NAME").ok_or("missing package name")?,
                env.get("CARGO_PKG_VERSION").ok_or("missing package version")?
            ],
            "root": identify(source)?,
            "inputs": ids,
            "test": args.iter().any(|s| s == "--test"),
            "explicitCfg": cfg,
            "externAliases": aliases
        }));
    }
    if rows.is_empty() {
        return Err("no nightly Rust invocations recorded".into());
    }
    let mut all = BTreeSet::new();
    depfiles(target, &mut all)?;
    if seen != all {
        return Err("nightly Cargo emitted unpaired dep-info".into());
    }
    rows.sort_by_cached_key(|v| crate::ascii(&v.to_string()));
    Ok(rows)
}
