use provider_validation::{Result, cli, command, pin, read, sha, write};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
mod controls;
mod package;
const PREFIX: &str = "urn:telemetry-queue:";
struct Suite {
    cli: PathBuf,
    root: PathBuf,
    fixture: PathBuf,
}
impl Suite {
    fn save(&self, name: &str, doc: &Value) -> Result<PathBuf> {
        let file = self.root.join(format!("{name}.json"));
        write(&file, doc)?;
        Ok(file)
    }
    fn digest(&self, verb: &str, file: &Path, error: Option<&str>) -> Result<String> {
        let mut cmd = Command::new(&self.cli);
        cmd.args(["sequence-journal", verb]).arg(file);
        Ok(
            String::from_utf8(command(&mut cmd, &self.root, &[], error)?)?
                .trim()
                .to_owned(),
        )
    }
    fn build(
        &self,
        graph: &Path,
        overlay: &Path,
        name: &str,
        gp: &str,
        bp: &str,
        expected: Option<&str>,
    ) -> Result<PathBuf> {
        let dest = self.root.join(name);
        let mut cmd = Command::new(&self.cli);
        cmd.args(["build", "sequence-journal", "rust"])
            .arg(graph)
            .arg(overlay)
            .arg(&dest)
            .args(["--graph-sha256", gp, "--overlay-sha256", bp]);
        command(&mut cmd, &self.root, &[], expected)?;
        Ok(dest)
    }
    fn generate(&self, graph: &Path, overlay: &Path, name: &str) -> Result<PathBuf> {
        let g = self.digest("digest", graph, None)?;
        let b = self.digest("overlay-digest", overlay, None)?;
        let out = self.build(graph, overlay, name, &g, &b, None)?;
        let lock = read(&out.join("projection.lock"))?;
        assert_eq!(lock["graphSha256"], g);
        assert_eq!(lock["overlaySha256"], b);
        assert_eq!(lock["sourceSha256"], sha(&out.join("sequence_journal.rs"))?);
        assert_eq!(
            lock["obligationsSha256"],
            sha(&out.join("obligations.json"))?
        );
        Ok(out)
    }
    fn native(
        &self,
        dir: &Path,
        action: &str,
        release: bool,
        error: Option<&str>,
    ) -> Result<String> {
        let mut cmd = Command::new("cargo");
        cmd.args([action, "--locked", "--offline", "--quiet", "--target-dir"])
            .arg(dir.join("target"));
        if release {
            cmd.arg("--release");
        }
        Ok(String::from_utf8(command(&mut cmd, dir, &[], error)?)?)
    }
    fn consumer(&self, projection: &Path, name: &str, error: Option<&str>) -> Result<PathBuf> {
        let dir = self.root.join(name);
        fs::create_dir_all(dir.join("src"))?;
        for name in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
            fs::copy(self.fixture.join(name), dir.join(name))?;
        }
        fs::copy(
            projection.join("sequence_journal.rs"),
            dir.join("src/journal.rs"),
        )?;
        self.native(&dir, "run", true, error)?;
        Ok(dir)
    }
    fn append_lexical(&self, path: &Path) -> Result {
        let mut file = fs::OpenOptions::new().append(true).open(path)?;
        file.write_all(b"\n")?;
        file.write_all(&fs::read(self.fixture.join("lexical.rs"))?)?;
        Ok(())
    }
}
fn op<'a>(g: &'a mut Value, name: &str) -> &'a mut Value {
    let id = format!("{PREFIX}{name}");
    g["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|o| o["id"] == id)
        .unwrap()
}
fn obligations(graph: &Value, doc: &Value) {
    let mut operations = BTreeSet::new();
    let mut cases = BTreeSet::new();
    for op in graph["operations"].as_array().unwrap() {
        let id = op["id"].as_str().unwrap();
        operations.insert(id);
        let ordered: Vec<_> = op["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        cases.extend(ordered.iter().copied());
        assert_eq!(doc["operations"][id]["orderedCases"], json!(ordered));
    }
    for (key, expected) in [("operations", operations), ("cases", cases)] {
        let actual: BTreeSet<_> = doc[key]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected);
    }
    assert_eq!(
        doc["cases"][format!("{PREFIX}probe_evict_rollback:0")]["effectFailures"],
        json!([
        {"effect":"evict-receipt","error":"urn:telemetry-queue:Bounds","state":"unchanged"},
        {"effect":"append","error":"urn:telemetry-queue:Bounds","state":"unchanged"}])
    );
}
fn main() -> Result {
    let work = tempfile::tempdir()?;
    let s = Suite {
        cli: cli()?,
        root: work.path().canonicalize()?,
        fixture: Path::new(env!("CARGO_MANIFEST_DIR")).join("../sequence-journal/fixture"),
    };
    let graph = s.fixture.join("graph.json");
    let overlay = s.fixture.join("rust.json");
    let raw = read(&graph)?;
    let binding = read(&overlay)?;
    let positive = s.generate(&graph, &overlay, "positive")?;
    obligations(&raw, &read(&positive.join("obligations.json"))?);
    let mut equivalent = raw.clone();
    for key in ["errors", "operations"] {
        equivalent[key].as_array_mut().unwrap().reverse();
    }
    equivalent["limits"]["max-cursor"]["cursor"] = json!(7);
    equivalent["states"][0]["initialCursor"] = json!(0);
    let repeat = s.generate(&s.save("equivalent", &equivalent)?, &overlay, "replay")?;
    for name in ["sequence_journal.rs", "projection.lock", "obligations.json"] {
        assert_eq!(fs::read(positive.join(name))?, fs::read(repeat.join(name))?);
    }
    s.consumer(&positive, "baseline", None)?;
    let mut changed = raw.clone();
    op(&mut changed, "post")["cases"]
        .as_array_mut()
        .unwrap()
        .swap(1, 5);
    let reordered = s.generate(&s.save("changed-order", &changed)?, &overlay, "reordered")?;
    s.consumer(&reordered, "ordering-control", Some("left: Err(Gap)"))?;
    let mut larger = raw.clone();
    larger["limits"]["max-records"]["size"] = json!(3);
    let increased = s.generate(&s.save("larger", &larger)?, &overlay, "increased")?;
    s.consumer(&increased, "capacity-control", Some("left: 1"))?;
    let mut flat = binding.clone();
    flat["storage"] = read(&s.fixture.join("storage.json"))?;
    let flat_path = s.save("flat-overlay", &flat)?;
    let projected = s.generate(&graph, &flat_path, "flat-generated")?;
    let flat_crate = s.consumer(&projected, "flat-public", None)?;
    let mut mixed = flat.clone();
    mixed["storage"]
        .as_object_mut()
        .unwrap()
        .remove(&format!("{PREFIX}near-cursor-limit"))
        .unwrap();
    let mixed = s.generate(&graph, &s.save("mixed-overlay", &mixed)?, "mixed-generated")?;
    s.consumer(&mixed, "mixed-public", None)?;
    let mut empty = binding.clone();
    empty["storage"] = json!({});
    assert_eq!(
        s.digest("overlay-digest", &s.save("empty-storage", &empty)?, None)?,
        s.digest("overlay-digest", &overlay, None)?
    );
    s.append_lexical(&flat_crate.join("src/journal.rs"))?;
    assert!(
        s.native(&flat_crate, "test", true, None)?
            .contains("2 passed; 0 failed")
    );
    fs::OpenOptions::new()
        .append(true)
        .open(flat_crate.join("src/main.rs"))?
        .write_all(
            b"
#[allow(dead_code)]
fn read_private(queue: &journal::TelemetryQueue) -> usize {
    queue.confirmations.len()
}
",
        )?;
    s.native(&flat_crate, "check", false, Some("field `confirmations`"))?;
    controls::verify(&s, &raw, &binding, &flat)?;
    package::verify(&s, &raw, &binding, &flat_path, &positive)?;
    println!(
        "Sequence journal passed 256 native boundary cases, atomic rollback, order and capacity mutations, and package composition"
    );
    Ok(())
}
