use super::*;
fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    ensure!(
        result.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(result.stdout)
}
fn rev(dir: &Path, name: &str) -> Result<String> {
    Ok(String::from_utf8(git(dir, &["rev-parse", name])?)?
        .trim()
        .into())
}
fn tree(dir: &Path) -> Result<BTreeMap<String, String>> {
    let bytes = git(dir, &["ls-tree", "-r", "-z", "HEAD"])?;
    std::str::from_utf8(&bytes)?
        .split('\0')
        .filter(|r| !r.is_empty())
        .map(|r| {
            let (meta, path) = r.split_once('\t').context("invalid Git tree")?;
            Ok((path.into(), meta.into()))
        })
        .collect()
}
pub fn identities(upstream: &Path, candidate: &Path) -> Result<(String, String)> {
    ensure!(rev(upstream, "HEAD")? == PIN, "wrong original revision");
    ensure!(
        rev(upstream, "HEAD^{tree}")? == original::TREE,
        "wrong original tree"
    );
    let revision = rev(candidate, "HEAD")?;
    ensure!(
        valid_hash(&json!(revision), 40) && revision != PIN,
        "expected distinct candidate commit"
    );
    let before = tree(upstream)?;
    let after = tree(candidate)?;
    ensure!(
        before.keys().eq(after.keys()),
        "candidate changed tracked paths"
    );
    let changed: Set = before
        .iter()
        .filter(|(p, meta)| after.get(*p) != Some(*meta))
        .map(|(p, _)| p.clone())
        .collect();
    let evidence = candidate
        .parent()
        .context("missing candidate parent")?
        .join("first");
    let raw = read(&evidence.join("project.toml"))?;
    let pin = fs::read_to_string(root().join("examples/marmish/assembly-v2.sha256"))?;
    ensure!(
        Some(digest(&raw).as_str()) == pin.split_whitespace().next(),
        "candidate assembly manifest changed"
    );
    let manifest: Value = toml::from_str(std::str::from_utf8(&raw)?)?;
    let generated: Map = array(&manifest["files"])?
        .iter()
        .filter(|r| r["producer"] == "generated")
        .map(|r| Ok((text(&r["path"])?.into(), r.clone())))
        .collect::<Result<_>>()?;
    let generated_names: Set = generated.keys().cloned().collect();
    let frame = "crates/marmish-core/src/frame.rs";
    let mut allowed = set(&[
        "docs/ontology/code_references.jsonl",
        "docs/ontology/source_files.jsonl",
        "docs/ontology/symbols.jsonl",
    ]);
    allowed.extend(generated_names.iter().cloned());
    ensure!(
        generated_names == set(&[frame, "crates/marmish-core/src/lib.rs"])
            && generated_names.is_subset(&changed)
            && changed.is_subset(&allowed),
        "unexpected candidate source change"
    );
    for (path, row) in generated {
        let bytes = git(candidate, &["show", &format!("HEAD:{path}")])?;
        let folder = if path == frame {
            "framing-projection"
        } else {
            "facade-projection"
        };
        ensure!(
            row["sha256"] == digest(&bytes)
                && row["sha256"]
                    == digest(&read(&evidence.join(folder).join(text(&row["module"])?))?),
            "candidate module differs from locked producer: {path}"
        );
    }
    ensure!(
        changed.iter().all(|p| before[p]
            .split_whitespace()
            .take(2)
            .eq(after[p].split_whitespace().take(2))),
        "changed candidate modes"
    );
    Ok((revision, rev(candidate, "HEAD^{tree}")?))
}
pub fn collect(candidate: &Path, out: &Path, revision: &str) -> Result<()> {
    ensure!(
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
        "native Apple Silicon required"
    );
    fs::create_dir_all(out)?;
    ensure!(
        fs::read_dir(out)?.next().is_none(),
        "qualification requires an empty output directory"
    );
    original::recover(&out.join("original"))?;
    crate::baseline::run(candidate, &out.join("candidate"), revision, true)
}
