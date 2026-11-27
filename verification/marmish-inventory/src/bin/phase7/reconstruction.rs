use super::*;
use sha1::{Digest, Sha1};

const ONTOLOGY: [&str; 3] = [
    "docs/ontology/code_references.jsonl",
    "docs/ontology/source_files.jsonl",
    "docs/ontology/symbols.jsonl",
];
const PRODUCERS: [(&str, &str); 2] = [("framing", "frame.rs"), ("facade", "lib.rs")];
pub fn git_object(kind: &str, data: &[u8]) -> String {
    let mut hash = Sha1::new();
    hash.update(format!("{kind} {}\0", data.len()));
    hash.update(data);
    format!("{:x}", hash.finalize())
}
type Entries = BTreeMap<String, (String, String)>;
/// Git sorts a subtree as if its name has a trailing slash.
fn tree_hash(entries: &Entries) -> Result<String> {
    let mut dirs: BTreeMap<PathBuf, BTreeMap<Vec<u8>, Vec<u8>>> = BTreeMap::new();
    fn insert(
        dirs: &mut BTreeMap<PathBuf, BTreeMap<Vec<u8>, Vec<u8>>>,
        path: &Path,
        mode: &str,
        oid: &str,
    ) -> Result<()> {
        let name = path
            .file_name()
            .context("missing Git entry name")?
            .as_encoded_bytes();
        let mut key = name.to_vec();
        if mode == "40000" {
            key.push(b'/');
        }
        let mut record = mode.as_bytes().to_vec();
        record.push(b' ');
        record.extend(name);
        record.push(0);
        let hash = hex_decode(oid)?;
        ensure!(hash.len() == 20, "invalid Git object ID");
        record.extend(hash);
        dirs.entry(path.parent().unwrap().to_owned())
            .or_default()
            .insert(key, record);
        Ok(())
    }
    for (path, (mode, oid)) in entries {
        insert(&mut dirs, Path::new(path), mode, oid)?;
    }
    while let Some((path, records)) = dirs.pop_last() {
        let tree = git_object("tree", &records.into_values().flatten().collect::<Vec<_>>());
        if path.as_os_str().is_empty() {
            return Ok(tree);
        }
        insert(&mut dirs, &path, "40000", &tree)?;
    }
    Ok(git_object("tree", &[]))
}
fn commit_payload(tree: &str) -> Vec<u8> {
    let id = "Oddlang qualification <qualification@oddlang.invalid> 1795640010 -0700";
    format!("tree {tree}\nauthor {id}\ncommitter {id}\n\nQualify regenerated Marmish framing and facade from {PIN}\n").into_bytes()
}
fn immutable(base: &Path, mut entries: Entries, result: &Value) -> Result<Map> {
    let dir = base.join("immutable");
    let names: Set = std::iter::once("commit.raw")
        .chain(ONTOLOGY.iter().map(|p| p.rsplit('/').next().unwrap()))
        .map(str::to_owned)
        .collect();
    ensure!(
        fs::symlink_metadata(&dir)?.is_dir() && filenames(&dir)? == names,
        "incomplete immutable Git object witnesses"
    );
    let mut hashes = Map::new();
    for path in ONTOLOGY {
        let bytes = read(&dir.join(Path::new(path).file_name().unwrap()))?;
        let (_, oid) = entries
            .get_mut(path)
            .context("missing original ontology file")?;
        let new_oid = git_object("blob", &bytes);
        ensure!(
            *oid != new_oid,
            "unchanged ontology blob labelled regenerated: {path}"
        );
        *oid = new_oid;
        hashes.insert(path.into(), json!(digest(&bytes)));
    }
    let tree = tree_hash(&entries)?;
    ensure!(
        result["candidateTree"] == tree,
        "candidate Git tree disagrees with final ontology and assembled source blobs"
    );
    let raw = read(&dir.join("commit.raw"))?;
    ensure!(
        raw == commit_payload(&tree) && result["candidateCommit"] == git_object("commit", &raw),
        "candidate Git commit disagrees with archived immutable object or deterministic build recipe"
    );
    Ok(hashes)
}
pub fn check_source_rows(rows: &[Value], files: &Map, expected: &Set) -> Result<()> {
    let paths: Set = rows
        .iter()
        .map(|r| text(&r["path"]).map(str::to_owned))
        .collect::<Result<_>>()?;
    ensure!(
        rows.len() == files.len() && paths == files.keys().cloned().collect(),
        "assembly paths differ from original inventory"
    );
    let mut generated = Set::new();
    for row in rows {
        let path = text(&row["path"])?;
        ensure!(
            row["mode"] == files[path]["mode"],
            "assembly mode differs: {path}"
        );
        if row["producer"] == "generated" {
            generated.insert(path.to_owned());
        } else {
            ensure!(
                row["producer"] == "retained" && row["sha256"] == files[path]["sha256"],
                "retained source digest differs: {path}"
            );
        }
    }
    ensure!(&generated == expected, "unexpected generated module set");
    Ok(())
}
pub fn verified_summary(dir: &Path, inventory: &Value, release_dir: &Path) -> Result<Value> {
    let generated_paths = PRODUCERS
        .iter()
        .map(|(_, m)| format!("crates/marmish-core/src/{m}"))
        .collect();
    let pin = String::from_utf8(read(&release_dir.join("assembly-v2.sha256"))?)?;
    let pin = pin
        .split_whitespace()
        .next()
        .context("missing assembly manifest pin")?;
    let files = object(&inventory["files"])?;
    let mut previous = None;
    for build in ["first", "second"] {
        let base = dir.join(build);
        let manifest = read(&base.join("project.toml"))?;
        ensure!(
            digest(&manifest) == pin,
            "assembly manifest differs from frozen release"
        );
        let manifest: Value = toml::from_str(std::str::from_utf8(&manifest)?)?;
        let rows = array(&manifest["files"])?;
        check_source_rows(rows, files, &generated_paths)?;
        let mut entries: Entries = files
            .iter()
            .map(|(p, f)| {
                Ok((
                    p.clone(),
                    (text(&f["mode"])?.into(), text(&f["gitBlob"])?.into()),
                ))
            })
            .collect::<Result<_>>()?;
        let mut generated = Map::new();
        for (name, module) in PRODUCERS {
            let release = read_json(&release_dir.join(format!("{name}-release.json")))?;
            let imported = base.join(name);
            let projected = base.join(format!("{name}-projection"));
            let lock = read(&imported.join("import.lock"))?;
            ensure!(
                release["importSha256"] == digest(&lock),
                "{build} {name} unpinned input"
            );
            let lock: Value = serde_json::from_slice(&lock)?;
            let hashes = file_hashes(&imported, &["source-map.json", "domain.ttl"])?;
            ensure!(
                lock["files"] == hashes
                    && lock["graphSha256"] == hashes["domain.ttl"]
                    && hashes["domain.ttl"] == release["graphSha256"],
                "{build} {name} import bytes disagree with lock"
            );
            let receipt = read(&projected.join("projection.json"))?;
            ensure!(
                release["projectionSha256"] == digest(&receipt),
                "{build} {name} unpinned projection"
            );
            let path = format!("crates/marmish-core/src/{module}");
            let row = rows
                .iter()
                .find(|r| r["path"] == path)
                .context("missing generated module")?;
            let receipt: Value = serde_json::from_slice(&receipt)?;
            ensure!(
                receipt
                    == json!({"version":1,"graphSha256":release["graphSha256"],"files":{module:row["sha256"]}}),
                "projection receipt disagrees with assembly"
            );
            let bytes = read(&projected.join(module))?;
            ensure!(
                row["sha256"] == digest(&bytes),
                "generated output differs from producer receipt: {path}"
            );
            let blob = git_object("blob", &bytes);
            generated.insert(path.clone(),json!({"originalBlob":entries[&path].1,"candidateBlob":blob,"sha256":row["sha256"]}));
            entries.insert(path, (text(&row["mode"])?.into(), blob));
            if name == "framing" {
                ensure!(
                    release["witSha256"] == digest(&read(&projected.join("world.wit"))?),
                    "framing interface changed"
                );
            }
        }
        let result = read_json(&base.join("result.json"))?;
        let ontologies = strings(&result["regeneratedOntology"])?;
        ensure!(
            result["assemblySha256"] == pin
                && result["generated"] == json!(generated)
                && result["sourceFiles"] == rows.len()
                && result["retainedFiles"] == rows.len() - generated.len()
                && result["stagedAssemblyTree"] == tree_hash(&entries)?
                && ontologies.len() == ONTOLOGY.len()
                && ontologies.into_iter().collect::<Set>()
                    == ONTOLOGY.map(str::to_owned).into_iter().collect()
                && ["candidateCommit", "candidateTree"]
                    .iter()
                    .all(|k| valid_hash(&result[k], 40)),
            "{build} result disagrees with raw assembly evidence"
        );
        let hashes = immutable(&base, entries, &result)?;
        let check = String::from_utf8(read(&base.join("ontology-check.txt"))?)?;
        let update = String::from_utf8(read(&base.join("ontology-update.txt"))?)?;
        ensure!(
            check.starts_with("code ontology check: ")
                && update.replacen("ontology update:", "ontology check:", 1) == check,
            "missing ontology check or update observations"
        );
        let evidence = (result, update, check, hashes);
        if let Some(prior) = &previous {
            ensure!(prior == &evidence, "independent rebuild results differ");
        }
        previous = Some(evidence);
    }
    let mut expected = previous.unwrap().0;
    expected["originalCommit"] = json!(PIN);
    expected["independentBuilds"] = json!(2);
    Ok(expected)
}
pub fn build_pair(dir: &Path, inventory: &Value, release_dir: &Path) -> Result<Value> {
    let expected = verified_summary(dir, inventory, release_dir)?;
    let summary = read_json(&dir.join("rebuild.json"))?;
    ensure!(
        summary == expected,
        "rebuild summary disagrees with independent runs"
    );
    Ok(summary)
}
