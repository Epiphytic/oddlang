use crate::*;
use sha1::{Digest, Sha1};
use syntax::Syntax;

pub fn classify_file(path: &str) -> Result<&'static str> {
    let p = Path::new(path);
    let parts: Vec<_> = path.split('/').collect();
    let name = parts.last().copied().unwrap_or_default();
    let top = parts[0];
    let ext = p.extension().and_then(|x| x.to_str()).unwrap_or_default();
    Ok(
        if path == "scripts/install.sh"
            || (p.parent() == Some(Path::new("docs/ontology")) && matches!(ext, "jsonl" | "yaml"))
        {
            "generated-artifact"
        } else if top == "tests" && parts.contains(&"fixtures") {
            "test-fixture"
        } else if matches!(
            name,
            "Cargo.toml"
                | "Cargo.lock"
                | "rust-toolchain.toml"
                | "build.rs"
                | "uv.lock"
                | "pyproject.toml"
                | "package.json"
                | "package-lock.json"
        ) || matches!(top, ".github" | "patches")
        {
            "build-input"
        } else if top == "tests" || (top == "crates" && parts.contains(&"tests")) {
            "test"
        } else if top == "crates" && ext == "rs" {
            "cargo-source"
        } else if matches!(top, "scripts" | "tools") {
            if name.starts_with("test_")
                || name.starts_with("test-")
                || name.contains(".test.")
                || name.ends_with("-test.c")
                || name == "test.sh"
            {
                "test"
            } else {
                "tooling"
            }
        } else if matches!(top, "docs" | ".beads")
            || matches!(
                path,
                "AGENTS.md" | "LICENSE" | "README.md" | ".gitattributes" | ".gitignore"
            )
        {
            "static-asset"
        } else {
            bail!("unclassified Git path: {path}")
        },
    )
}
pub fn files(root: &Path, pin: &str) -> Result<Map> {
    let head = String::from_utf8(cargo::run(root, "git", &["rev-parse", "HEAD"], None)?)?;
    ensure!(
        head.trim() == pin,
        "upstream revision differs from project manifest"
    );
    ensure!(
        cargo::run(
            root,
            "git",
            &["status", "--porcelain", "--untracked-files=all"],
            None
        )?
        .is_empty(),
        "expected pristine pinned source checkout"
    );
    let tree = cargo::run(root, "git", &["ls-tree", "-rz", pin], None)?;
    let mut out = Map::new();
    for row in tree.split(|&b| b == 0).filter(|s| !s.is_empty()) {
        let (meta, path) = std::str::from_utf8(row)?
            .split_once('\t')
            .context("invalid git tree")?;
        let f: Vec<_> = meta.split_whitespace().collect();
        ensure!(
            f.len() == 3 && f[1] == "blob" && matches!(f[0], "100644" | "100755"),
            "unsupported Git entry {path}"
        );
        let data = fs::read(root.join(path))?;
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", data.len()));
        hash.update(&data);
        ensure!(
            format!("{:x}", hash.finalize()) == f[2],
            "checkout differs from pinned blob: {path}"
        );
        out.insert(
            path.into(),
            json!({"mode":f[0],"gitBlob":f[2],"sha256":digest(&data),"class":classify_file(path)?}),
        );
    }
    Ok(out)
}
pub fn scope<'a>(
    path: &str,
    r: &Value,
    ids: &'a BTreeMap<String, Vec<String>>,
) -> Result<Option<&'a str>> {
    let ordinal = number(&r[0])?;
    let line = number(&r[3])?;
    let local = ids.get(path).context("missing reference path")?;
    ensure!(
        ordinal <= local.len()
            && line > 0
            && !string(&r[1])?.is_empty()
            && !string(&r[2])?.is_empty(),
        "invalid lexical reference: {path}:{line} scope {ordinal}"
    );
    Ok(ordinal.checked_sub(1).map(|n| local[n].as_str()))
}
fn normalize(path: &Path) -> String {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
                if out.file_name().is_some()
                    && out.file_name() != Some(std::ffi::OsStr::new("..")) =>
            {
                out.pop();
            }
            p => out.push(p.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}
pub fn contexts(
    files: &Map,
    targets: &Value,
    syntax: &BTreeMap<String, Syntax>,
) -> Result<(
    BTreeMap<String, Vec<Value>>,
    BTreeSet<(String, usize, String)>,
)> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut missing = BTreeSet::new();
    for (tid, t) in object(targets)? {
        let required = strings(&t["requiredFeatures"])?
            .into_iter()
            .map(|f| format!("#[cfg(feature = \"{f}\")]"))
            .collect::<Vec<_>>();
        let mut pending = vec![(string(&t["path"])?.to_owned(), true, required)];
        let mut seen = BTreeSet::new();
        while let Some((path, is_root, cfg)) = pending.pop() {
            if !seen.insert((path.clone(), cfg.clone())) {
                continue;
            }
            ensure!(files.contains_key(&path), "missing target input: {path}");
            out.entry(path.clone())
                .or_default()
                .push(json!({"target":tid,"attributes":cfg}));
            let p = Path::new(&path);
            let dir = p.parent().context("expected relative module")?;
            let Some(src) = syntax.get(&path) else {
                continue;
            };
            for edge in &src.edges {
                let line = number(&edge["line"])?;
                let mut paths = vec![];
                if edge["kind"] == "include" {
                    if let Some(explicit) = edge["path"].as_str() {
                        paths.push(dir.join(explicit));
                    }
                } else {
                    let mut base = if is_root
                        || p.file_name() == Some(std::ffi::OsStr::new("mod.rs"))
                        || edge["path"].is_string()
                    {
                        dir.to_owned()
                    } else {
                        p.with_extension("")
                    };
                    for part in strings(&edge["inline"])? {
                        base.push(part);
                    }
                    if let Some(explicit) = edge["path"].as_str() {
                        paths.push(base.join(explicit));
                    } else {
                        let child = base.join(string(&edge["name"])?);
                        paths.extend([child.with_extension("rs"), child.join("mod.rs")]);
                    }
                }
                let matches: Vec<_> = paths
                    .iter()
                    .map(|p| normalize(p))
                    .filter(|p| files.contains_key(p))
                    .collect();
                if matches.len() != 1 {
                    missing.insert((path.clone(), line, tid.clone()));
                    continue;
                }
                let attrs: Set = cfg.iter().cloned().chain(src.attrs(line)).collect();
                pending.push((
                    matches.into_iter().next().unwrap(),
                    false,
                    attrs.into_iter().collect(),
                ));
            }
        }
    }
    Ok((out, missing))
}
fn test_only(attrs: &[String]) -> bool {
    attrs.iter().any(|raw| {
        let a: String = raw.split_whitespace().collect();
        matches!(a.as_str(), "#[test]" | "#[cfg(test)]" | "#[tokio::test]")
            || a.starts_with("#[tokio::test(")
            || a.strip_prefix("#[cfg(all(")
                .and_then(|x| x.strip_suffix("))]"))
                .is_some_and(|x| x.split(',').any(|v| v == "test"))
    })
}
pub fn classification<'a>(
    path: &str,
    class: &'a str,
    contexts: &[Value],
    attrs: &[String],
    targets: &Value,
) -> Result<(&'a str, String)> {
    if !matches!(class, "cargo-source" | "test") || !path.starts_with("crates/") {
        return Ok((class, format!("tracked {class}")));
    }
    if contexts.is_empty() {
        return Ok((
            "unsupported",
            "Rust declaration without resolved Cargo module root".into(),
        ));
    }
    let mut all_test = true;
    let mut examples = true;
    let mut qualification = true;
    let mut release = true;
    for c in contexts {
        let t = &targets[string(&c["target"])?];
        let attrs = strings(&c["attributes"])?;
        all_test &= test_only(&attrs) || t["kind"] == "test";
        examples &= t["kind"] == "example";
        qualification &= attrs
            .iter()
            .any(|a| a.contains("feature = \"installed-checks\""));
        release &= t["name"] == "marmish-release-tool";
    }
    let (class, why) = if class == "test" || test_only(attrs) || all_test {
        (
            "test",
            "test target or explicit test-only declaration/module",
        )
    } else if examples {
        ("example", "Cargo example target")
    } else if qualification {
        ("qualification-tool", "Cargo installed-checks feature")
    } else if release {
        ("build-input", "release packaging target")
    } else {
        (
            "retained-native",
            "original native definition in Cargo source closure",
        )
    };
    Ok((class, why.to_owned()))
}
