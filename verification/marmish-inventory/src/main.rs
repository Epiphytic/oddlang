mod cargo;
mod platforms;
mod source;
mod syntax;
use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use flate2::{Compression, GzBuilder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
type Map = serde_json::Map<String, Value>;
type Set = BTreeSet<String>;
type Counts = BTreeMap<String, usize>;
fn increment(counts: &mut Counts, k: &str) {
    *counts.entry(k.into()).or_default() += 1;
}
fn string(v: &Value) -> Result<&str> {
    v.as_str().context("expected string")
}
fn object(v: &Value) -> Result<&Map> {
    v.as_object().context("expected object")
}
fn number(v: &Value) -> Result<usize> {
    Ok(usize::try_from(
        v.as_u64().context("expected unsigned integer")?,
    )?)
}
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("expected array")
}
fn strings(v: &Value) -> Result<Vec<String>> {
    array(v)?
        .iter()
        .map(|s| Ok(self::string(s)?.into()))
        .collect()
}
fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn encode(v: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(v).expect("JSON value");
    bytes.push(b'\n');
    bytes
}
fn jsonl(root: &Path, name: &str) -> Result<Vec<Value>> {
    fs::read_to_string(root.join("docs/ontology").join(format!("{name}.jsonl")))?
        .lines()
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}
fn generate(root: &Path, manifest: &str) -> Result<(Value, Value)> {
    let project: Value = toml::from_str(manifest)?;
    let manifest_hash = digest(manifest.as_bytes());
    let mut files = source::files(root, string(&project["git_revision"])?)?;
    let source_files = jsonl(root, "source_files")?;
    let mut symbols = jsonl(root, "symbols")?;
    let refs = jsonl(root, "code_references")?;
    let mut parse_counts = Counts::new();
    for row in &source_files {
        let path = string(&row["path"])?;
        let f = files
            .get_mut(path)
            .context("ontology file missing from Git tree")?;
        ensure!(
            f["sha256"] == row["sha256"]
                && f["mode"] == row["mode"]
                && f.get("parseStatus").is_none(),
            "ontology file mismatches Git tree: {path}"
        );
        f["language"] = row["language"].clone();
        f["parseStatus"] = row["parse_status"].clone();
        increment(&mut parse_counts, string(&row["parse_status"])?);
    }
    let (cargo, members) = cargo::inventory(root, &project)?;
    let targets = &cargo["targets"];
    let (syntax, workflows) = syntax::inspect(root, &files)?;
    let (mut contexts, missing) = source::contexts(&files, targets, &syntax)?;
    for (path, items) in &mut contexts {
        items.sort_by_cached_key(encode);
        files.get_mut(path).unwrap()["cargoContexts"] = json!(items);
    }
    let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unique = Set::new();
    let mut classes = Counts::new();
    for sym in &mut symbols {
        let path = string(&sym["path"])?;
        let id = string(&sym["id"])?;
        let line = number(&sym["start_line"])?;
        ensure!(
            unique.insert(id.into()) && line >= 1 && line <= number(&sym["end_line"])?,
            "invalid symbol: {id}"
        );
        ids.entry(path.into()).or_default().push(id.into());
        let attrs = syntax.get(path).map(|s| s.attrs(line)).unwrap_or_default();
        let ctx = contexts.get(path).map(Vec::as_slice).unwrap_or_default();
        let (class, reason) =
            source::classification(path, string(&files[path]["class"])?, ctx, &attrs, targets)?;
        increment(&mut classes, class);
        let tids: Set = ctx
            .iter()
            .map(|c| string(&c["target"]).map(str::to_owned))
            .collect::<Result<_>>()?;
        sym["classification"] = json!(class);
        sym["reason"] = json!(reason);
        sym["attributes"] = json!(attrs);
        sym["targets"] = json!(tids);
    }
    for sym in &symbols {
        if let Some(id) = sym["parent_id"].as_str().filter(|s| !s.is_empty()) {
            ensure!(
                ids[string(&sym["path"])?].iter().any(|i| i == id),
                "missing lexical symbol parent"
            );
        }
    }
    let mut ref_count = 0;
    for r in &refs {
        let path = string(&r["path"])?;
        ensure!(files.contains_key(path), "reference to missing file");
        ids.entry(path.into()).or_default();
        for reference in array(&r["references"])? {
            source::scope(path, reference, &ids)?;
            ref_count += 1;
        }
    }
    let mut gaps = vec![
        "Lexical callees, generated declarations and workspace-wide symbol reachability remain unresolved.".to_string(),
        "Source parser recovery and omission markers preclude a complete declaration denominator.".to_string(),
    ];
    if !missing.is_empty() {
        gaps.push(format!("{} unresolved target/module edges.", missing.len()));
    }
    if let Some(n) = classes.get("unsupported").filter(|n| **n > 0) {
        gaps.push(format!("{n} unsupported lexical declarations."));
    }
    let framing = cargo::framing(root, &members, &project)?;
    let host = string(&framing["compiler"])?
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .context("missing host")?;
    let platforms = strings(&project["targets"])?;
    let profiles = strings(&project["feature_profiles"])?;
    let mut cfg: BTreeMap<String, Set> = BTreeMap::new();
    for t in &platforms {
        cfg.insert(
            t.clone(),
            String::from_utf8(cargo::run(
                root,
                "rustc",
                &["--print", "cfg", "--target", t],
                None,
            )?)?
            .lines()
            .map(str::to_owned)
            .collect(),
        );
    }
    let denom = platforms::denominators(&symbols, &files, &platforms, &profiles)?;
    let (catalog, by_platform) =
        platforms::tests(&syntax, &symbols, &files, targets, &cfg, &profiles)?;
    let mut packages = Map::new();
    let mut common: Option<Set> = None;
    let mut sets = BTreeMap::new();
    for (t, rows) in &by_platform {
        packages.insert(
            t.clone(),
            platforms::packaging(t, &files, &workflows, targets)?,
        );
        let sites: Set = rows
            .iter()
            .map(|r| string(&r["test"]).map(str::to_owned))
            .collect::<Result<_>>()?;
        common = Some(match common {
            None => sites.clone(),
            Some(prev) => prev.intersection(&sites).cloned().collect(),
        });
        sets.insert(t, sites);
    }
    let common = common.context("expected at least one platform")?;
    let mut platform_summary = Map::new();
    for (t, rows) in &by_platform {
        let sites = &sets[t];
        let restricted: Set = sites.difference(&common).cloned().collect();
        let mut ignored = Set::new();
        let mut unknown = Set::new();
        for r in rows {
            if r["gate"] == "ignored" {
                ignored.insert(string(&r["test"])?.into());
            }
            if !array(&r["uncertainProfiles"])?.is_empty() {
                unknown.insert(string(&r["test"])?.into());
            }
        }
        platform_summary.insert(
            t.clone(),
            json!({
                "observedTestSites": sites.len(),
                "platformRestrictedSites": restricted,
                "ignoredSites": ignored,
                "unknownConfigurationSites": unknown,
                "packaging": packages[t]
            }),
        );
    }
    let mut file_classes = Counts::new();
    for f in files.values() {
        increment(&mut file_classes, string(&f["class"])?);
    }
    let runtime: Counts = ["imported", "retained-native", "unsupported"]
        .into_iter()
        .map(|k| (k.into(), classes.get(k).copied().unwrap_or(0)))
        .collect();
    let counts = json!({
        "files": files.len(),
        "fileClasses": file_classes,
        "ontologyFiles": source_files.len(),
        "parseStatus": parse_counts,
        "symbols": symbols.len(),
        "symbolClasses": classes,
        "lexicalReferences": ref_count,
        "cargoTargets": object(targets)?.len(),
        "workspaceMembers": members.len(),
        "externalPackages": object(&cargo["external"])?.len(),
        "runtimeLexicalSitesByDisposition": runtime
    });
    let mut archive = json!({
        "schemaVersion": 2,
        "gitRevision": project["git_revision"],
        "manifestSha256": manifest_hash,
        "scope": project["inventory"],
        "denominators": denom,
        "testSites": catalog,
        "testApplicability": by_platform,
        "workflowTestSteps": platforms::workflow_tests(&workflows)?,
        "platformPackaging": packages,
        "files": files,
        "cargo": cargo,
        "rustcCfg": cfg,
        "symbols": symbols,
        "moduleSyntax": syntax,
        "unresolvedModuleEdges": missing,
        "references": refs,
        "referenceScope": "ordinal zero = file; positive = one-based symbol in same path, preserving symbols array order",
        "gaps": gaps
    });
    let summary = json!({
        "schemaVersion": 2,
        "gitRevision": project["git_revision"],
        "counts": counts,
        "denominators": denom,
        "platformInventory": platform_summary,
        "sourceInventorySha256": digest(&encode(&archive)),
        "completeness": "open",
        "gaps": gaps,
        "framingCompilerCheck": framing["status"],
        "host": host,
        "unresolvedModuleEdges": missing.len(),
        "manifestSha256": manifest_hash
    });
    archive["framingCompilerCheck"] = framing;
    Ok((archive, summary))
}

#[derive(Parser)]
struct Args {
    upstream: PathBuf,
    output: PathBuf,
    #[arg(long)]
    require_complete: bool,
    /// Compare with a frozen summary, including host-specific fields on matching hosts.
    #[arg(long)]
    compare_summary: Option<PathBuf>,
}
fn require_complete(gaps: &Value) -> Result<()> {
    let gaps = strings(gaps)?;
    ensure!(
        gaps.is_empty(),
        "open completeness obligations: {}",
        gaps.join("; ")
    );
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    let manifest = include_str!("../../../examples/marmish/project.toml");
    let (archive, mut summary) = generate(&args.upstream.canonicalize()?, manifest)?;
    let mut gzip = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::new(6));
    gzip.write_all(&encode(&archive))?;
    let bytes = gzip.finish()?;
    summary["inventorySha256"] = json!(digest(&bytes));
    fs::create_dir_all(&args.output)?;
    fs::write(args.output.join("inventory.json.gz"), bytes)?;
    let mut text = serde_json::to_vec_pretty(&summary)?;
    text.push(b'\n');
    fs::write(args.output.join("inventory-summary.json"), text)?;
    if let Some(path) = args.compare_summary {
        let mut expected: Value = serde_json::from_slice(&fs::read(path)?)?;
        let mut actual = summary.clone();
        if expected["host"] != actual["host"] {
            for k in ["host", "inventorySha256"] {
                expected
                    .as_object_mut()
                    .context("expected summary")?
                    .remove(k);
                actual.as_object_mut().unwrap().remove(k);
            }
        }
        ensure!(actual == expected, "Marmish inventory or summary changed");
    }
    if args.require_complete {
        require_complete(&summary["gaps"])?;
    }
    Ok(())
}
#[cfg(test)]
mod tests;
