//! rustc --edition 2024 inventory.rs -o inventory
//! inventory ODDLANG_CHECKOUT COMPANION_CHECKOUT ORIGINAL_MARMISH_CHECKOUT
//! Lists tracked scripts and textual callers at HEAD, using Git blobs for provenance.
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::Path,
    process::Command,
};

fn git(root: &str, args: &[&str]) -> Vec<u8> {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("start git");
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn files(root: &str) -> BTreeMap<String, String> {
    String::from_utf8(git(root, &["ls-tree", "-r", "HEAD"]))
        .unwrap()
        .lines()
        .filter_map(|row| {
            let (meta, path) = row.split_once('\t')?;
            let hash = meta.strip_prefix("100")?.split_whitespace().last()?;
            Some((path.to_string(), hash.to_string()))
        })
        .collect()
}

fn script(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|s| s.to_str()),
        Some("sh" | "bash" | "py" | "js" | "mjs" | "ts" | "ps1" | "rb" | "pl")
    )
}

fn main() {
    let [odd, companion, original]: [String; 3] = env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .expect("three checkout paths");
    let upstream = files(&original);
    let upstream_hashes: BTreeSet<_> = upstream.values().collect();
    println!("repository\tpath\tgit-blob\torigin\ttextual caller files");
    for (name, root) in [("oddlang", odd), ("marmish-oddlang", companion)] {
        let tree = files(&root);
        // Token matching finds paths, basenames and Python module imports. A basename is
        // accepted only when it identifies one script globally or in the caller's directory.
        let mut aliases: BTreeMap<String, Vec<&String>> = BTreeMap::new();
        for path in tree.keys().filter(|p| script(p)) {
            let p = Path::new(path);
            for key in [path.as_str(), p.file_name().unwrap().to_str().unwrap()] {
                aliases.entry(key.into()).or_default().push(path);
            }
        }
        let mut callers: BTreeMap<&String, BTreeSet<String>> = BTreeMap::new();
        for path in tree.keys() {
            if !(script(path)
                || matches!(
                    Path::new(path).extension().and_then(|s| s.to_str()),
                    Some("md" | "rs" | "yml" | "yaml" | "toml")
                ))
            {
                continue;
            }
            // Read the checked-out version so the inventory can be refreshed before commit.
            let Ok(code) = std::fs::read_to_string(Path::new(&root).join(path)) else {
                continue;
            };
            if path.ends_with(".py") {
                for line in code.lines().map(str::trim) {
                    let module = line
                        .strip_prefix("from ")
                        .or_else(|| line.strip_prefix("import "))
                        .and_then(|s| s.split_whitespace().next())
                        .unwrap_or("");
                    if module.is_empty() {
                        continue;
                    }
                    let local = Path::new(path)
                        .parent()
                        .unwrap()
                        .join(format!("{module}.py"));
                    if let Some((candidate, _)) = tree.get_key_value(local.to_str().unwrap()) {
                        callers.entry(candidate).or_default().insert(path.clone());
                    }
                }
            }
            for token in code.split(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c))) {
                let key = token.trim_start_matches("./");
                if key.is_empty() {
                    continue;
                }
                if let Some(matches) = aliases.get(key) {
                    for &candidate in matches {
                        if candidate != path
                            && (matches.len() == 1
                                || Path::new(candidate).parent() == Path::new(path).parent())
                        {
                            callers.entry(candidate).or_default().insert(path.clone());
                        }
                    }
                }
            }
        }
        for (path, hash) in tree.iter().filter(|(p, _)| script(p)) {
            let origin = if name == "oddlang" {
                "project"
            } else if upstream_hashes.contains(hash) {
                "inherited-identical"
            } else if upstream.contains_key(path) {
                "upstream-path-modified"
            } else {
                "project"
            };
            let refs = callers
                .get(path)
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(",");
            let refs = if refs.is_empty() { "-" } else { &refs };
            println!("{name}\t{path}\t{hash}\t{origin}\t{refs}");
        }
    }
}
