use super::*;
use rust_inventory::{commands, flag};

pub fn invocation(log: &str, executable: &str, name: &str) -> Result<Vec<String>> {
    let mut matches = Vec::new();
    for words in commands(log)? {
        if let Some(index) = words.iter().position(|w| w == executable) {
            let args = &words[index + 1..];
            if flag(args, "--crate-name") == [name] {
                matches.push(args.to_vec());
            }
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "expected one {executable} invocation for {name}, found {}",
            matches.len()
        )
        .into());
    }
    Ok(matches.remove(0))
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v
}

pub fn profile(args: &[String], root: &Path) -> Value {
    let excluded = [
        "metadata",
        "extra-filename",
        "embed-bitcode",
        "strip",
        "debuginfo",
        "split-debuginfo",
        "linker",
        "incremental",
    ];
    let codegen: Vec<_> = flag(args, "-C")
        .into_iter()
        .filter(|s| !excluded.contains(&s.split('=').next().unwrap()))
        .collect();
    let path_maps: Vec<_> = args
        .iter()
        .filter(|s| s.starts_with("--remap-path-prefix="))
        .map(|s| s.replace(root.to_str().unwrap(), "$WORKSPACE"))
        .collect();
    json!({
        "edition":args.iter().filter(|s|s.starts_with("--edition=")).collect::<Vec<_>>(),
        "crateType":flag(args,"--crate-type"),"target":flag(args,"--target"),"crateName":flag(args,"--crate-name"),
        "explicitCfg":sorted(flag(args,"--cfg")),"checkCfg":sorted(flag(args,"--check-cfg")),"codegen":codegen,
        "externAliases":sorted(flag(args,"--extern").into_iter().map(|x|x.split('=').next().unwrap()).collect()),
        "pathMaps":path_maps
    })
}

pub fn compare_profiles(left: &Value, right: &Value) -> Result<()> {
    for (key, expected) in left.as_object().ok_or("invalid compiler profile")? {
        if right.get(key) != Some(expected) {
            return Err(format!(
                "observer profile changed {key}: {expected} vs {}",
                right[key]
            )
            .into());
        }
    }
    Ok(())
}

pub fn bindings(
    log: &str,
    rustc: &str,
    selected: &[String],
    packages: &BTreeMap<PathBuf, Value>,
) -> Result<Value> {
    let mut producers: BTreeMap<(PathBuf, String), Vec<(PathBuf, Value)>> = BTreeMap::new();
    for words in commands(log)? {
        let Some(index) = words.iter().position(|w| w == rustc) else {
            continue;
        };
        let args = &words[index + 1..];
        let kinds = flag(args, "--crate-type");
        if !kinds
            .iter()
            .any(|k| matches!(*k, "lib" | "rlib" | "dylib" | "proc-macro"))
        {
            continue;
        }
        let exactly = |f: &str| -> Result<&str> {
            let v = flag(args, f);
            if v.len() != 1 {
                return Err(format!("expected one {f}").into());
            }
            Ok(v[0])
        };
        let name = exactly("--crate-name")?;
        let out = PathBuf::from(exactly("--out-dir")?);
        let suffixes: Vec<_> = flag(args, "-C")
            .into_iter()
            .filter_map(|v| v.strip_prefix("extra-filename="))
            .collect();
        if suffixes.len() > 1 {
            return Err(format!("ambiguous output suffix for {name}").into());
        }
        let dir = words[..index]
            .iter()
            .find_map(|s| s.strip_prefix("CARGO_MANIFEST_DIR="))
            .ok_or("missing Cargo manifest directory")?;
        let dir = PathBuf::from(dir);
        let evidence = json!({"crate":name,"crateTypes":sorted(kinds),"target":flag(args,"--target"),"explicitCfg":sorted(flag(args,"--cfg"))});
        producers
            .entry((out, format!("{name}{}", suffixes.first().unwrap_or(&""))))
            .or_default()
            .push((dir.canonicalize().unwrap_or(dir), evidence));
    }
    let mut result = json!({});
    for arg in flag(selected, "--extern") {
        let (alias, file) = arg.split_once('=').ok_or("missing extern artifact path")?;
        if result.get(alias).is_some() {
            return Err(format!("missing or duplicate extern binding: {alias}").into());
        }
        let file = Path::new(file);
        if !matches!(
            file.extension().and_then(|x| x.to_str()),
            Some("rmeta" | "rlib" | "dylib" | "so" | "dll")
        ) {
            return Err(format!("unsupported extern artifact: {}", file.display()).into());
        }
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("missing extern artifact stem")?;
        let stems: BTreeSet<_> = [stem, stem.strip_prefix("lib").unwrap_or(stem)].into();
        let found: Vec<_> = stems
            .iter()
            .filter_map(|s| {
                producers.get(&(
                    file.parent().unwrap_or(Path::new("")).to_owned(),
                    s.to_string(),
                ))
            })
            .flatten()
            .collect();
        if found.len() != 1 {
            return Err(format!(
                "expected one build producer for extern {alias}, found {}",
                found.len()
            )
            .into());
        }
        let (dir, record) = found[0];
        result[alias] = record.clone();
        result[alias]["package"] = packages
            .get(dir)
            .ok_or_else(|| format!("unknown Cargo package producing extern {alias}"))?
            .clone();
    }
    Ok(result)
}
