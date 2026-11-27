//! Materialize the published one- or two-producer Marmish assembly plan from a pinned Git tree.
use provider_validation::{Result, read, sha};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    fmt::Write as _,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const FRAME: &str = "crates/marmish-core/src/frame.rs";
const FACADE: &str = "crates/marmish-core/src/lib.rs";
const TARGET: &str = "aarch64-apple-darwin";

#[derive(Deserialize)]
struct Project {
    git_revision: String,
    workspace_members: Vec<String>,
}
struct Producer<'a> {
    imported: &'a Path,
    projected: &'a Path,
    graph: String,
    import: String,
    projection: String,
}
impl<'a> Producer<'a> {
    fn load(imported: &'a Path, projected: &'a Path) -> Result<Self> {
        Ok(Self {
            imported,
            projected,
            graph: sha(&imported.join("domain.ttl"))?,
            import: sha(&imported.join("import.lock"))?,
            projection: sha(&projected.join("projection.json"))?,
        })
    }
    fn pins(&self) -> Value {
        json!({"graphSha256":self.graph,"importSha256":self.import,"projectionSha256":self.projection})
    }
    fn import_into(&self, store: &Path) -> Result {
        let dir = store.join("imports").join(&self.import);
        copy_tree(self.imported, &dir)?;
        require(
            sha(&dir.join("domain.ttl"))? == self.graph
                && sha(&dir.join("import.lock"))? == self.import,
            "import changed during materialization",
        )
    }
    fn projection_dir(&self, store: &Path) -> PathBuf {
        store.join("projections").join(&self.projection)
    }
    fn write_pins(&self, out: &mut String) -> Result {
        for (key, val) in [
            ("graphSha256", &self.graph),
            ("importSha256", &self.import),
            ("projectionSha256", &self.projection),
        ] {
            field(out, key, val)?;
        }
        Ok(())
    }
}
fn require(condition: bool, error: impl Into<String>) -> Result {
    if condition {
        Ok(())
    } else {
        Err(error.into().into())
    }
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn copy_tree(src: &Path, dst: &Path) -> Result {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            fs::copy(path, target)?;
        }
    }
    Ok(())
}
fn field(text: &mut String, name: &str, value: &str) -> Result {
    writeln!(text, "{name} = {}", serde_json::to_string(value)?)?;
    Ok(())
}
enum ProducerKind {
    Retained,
    Frame,
    Facade,
}
struct FileRow {
    path: String,
    mode: String,
    hash: String,
    producer: ProducerKind,
}
impl FileRow {
    fn emit(&self, text: &mut String, v2: bool) -> Result {
        text.push_str("\n[[files]]\n");
        field(text, "path", &self.path)?;
        field(text, "mode", &self.mode)?;
        match self.producer {
            ProducerKind::Retained => {
                field(text, "producer", "retained")?;
                field(text, "sha256", &self.hash)?;
            }
            ProducerKind::Frame => {
                field(text, "producer", "generated")?;
                field(text, "module", "frame.rs")?;
                field(text, "sha256", &self.hash)?;
                if v2 {
                    field(text, "emitter", "framing-v1")?;
                }
            }
            ProducerKind::Facade => {
                field(text, "producer", "generated")?;
                field(text, "sha256", &self.hash)?;
                field(text, "module", "lib.rs")?;
                field(text, "emitter", "rust-facade-v1")?;
            }
        }
        Ok(())
    }
}
fn retain(upstream: &Path, revision: &str, retained: &Path, frame: &str) -> Result<Vec<FileRow>> {
    fs::create_dir_all(retained)?;
    let tree = Command::new("git")
        .arg("-C")
        .arg(upstream)
        .args(["ls-tree", "-r", "-z", revision])
        .output()?;
    require(tree.status.success(), String::from_utf8_lossy(&tree.stderr))?;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(upstream)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut rows = Vec::new();
    let result = (|| -> Result {
        for entry in tree.stdout.split(|&c| c == 0).filter(|e| !e.is_empty()) {
            let (info, path) = std::str::from_utf8(entry)?
                .split_once('\t')
                .ok_or("invalid Git tree entry")?;
            let parts: Vec<_> = info.split_ascii_whitespace().collect();
            let [mode, "blob", oid] = parts.as_slice() else {
                return Err(format!("unsupported Git tree entry at {path}").into());
            };
            require(
                matches!(*mode, "100644" | "100755"),
                format!("unsupported Git mode at {path}"),
            )?;
            let (hash, producer) = if path == FRAME {
                (frame.into(), ProducerKind::Frame)
            } else {
                writeln!(input, "{oid}")?;
                input.flush()?;
                let mut header = String::new();
                output.read_line(&mut header)?;
                let parts: Vec<_> = header.split_ascii_whitespace().collect();
                let [actual, "blob", length] = parts.as_slice() else {
                    return Err(format!("expected Git blob at {path}").into());
                };
                require(actual == oid, format!("unexpected Git object at {path}"))?;
                let mut bytes = vec![0; length.parse()?];
                output.read_exact(&mut bytes)?;
                let mut delimiter = [0];
                output.read_exact(&mut delimiter)?;
                require(
                    delimiter == [b'\n'],
                    format!("truncated Git object at {path}"),
                )?;
                let hash = digest(&bytes);
                let target = retained.join(&hash);
                if !target.exists() {
                    fs::write(target, bytes)?;
                }
                (hash, ProducerKind::Retained)
            };
            rows.push(FileRow {
                path: path.into(),
                mode: (*mode).into(),
                hash,
                producer,
            });
        }
        Ok(())
    })();
    drop(input);
    drop(output);
    let status = child.wait()?;
    result?;
    require(status.success(), "Git object reader failed")?;
    require(
        rows.iter().any(|r| r.path == FRAME),
        "missing generated framing destination",
    )?;
    Ok(rows)
}
fn verify_plan(examples: &Path, name: &str, body: &str) -> Result {
    let pin = fs::read_to_string(examples.join(name))?;
    require(
        Some(digest(body.as_bytes()).as_str()) == pin.split_whitespace().next(),
        "complete assembly plan differs from its published digest",
    )
}
fn header(project: &Project, version: u8) -> Result<String> {
    let crates = project
        .workspace_members
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut text = format!("version = {version}\ncrates = [{}]\n", crates.join(", "));
    field(&mut text, "target", TARGET)?;
    text.push_str("features = []\n");
    Ok(text)
}
fn materialize(upstream: &Path, frame: Producer, facade: Option<Producer>, out: &Path) -> Result {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let examples = root.join("examples/marmish");
    let project: Project = toml::from_str(&fs::read_to_string(examples.join("project.toml"))?)?;
    let mut expected = frame.pins();
    expected["sourceRevision"] = json!(project.git_revision);
    expected["witSha256"] = json!(sha(&frame.projected.join("world.wit"))?);
    require(
        expected == read(&examples.join("framing-release.json"))?,
        "Marmish inputs differ from the published framing release",
    )?;
    let manifest = fs::read(frame.projected.join("projection.json"))?;
    let projection: Value = serde_json::from_slice(&manifest)?;
    let generated = fs::read(frame.projected.join("frame.rs"))?;
    let frame_hash = digest(&generated);
    require(
        digest(&manifest) == frame.projection
            && projection["graphSha256"] == frame.graph
            && projection["files"] == json!({"frame.rs":frame_hash}),
        "unexpected framing graph, module set or module digest",
    )?;
    if out.exists() {
        require(
            out.is_dir() && fs::read_dir(out)?.next().is_none(),
            "materialization requires empty output",
        )?;
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::tempdir_in(parent)?;
    let store = stage.path().join("store");
    frame.import_into(&store)?;
    let dest = frame.projection_dir(&store);
    fs::create_dir_all(&dest)?;
    fs::write(dest.join("projection.json"), manifest)?;
    fs::write(dest.join("frame.rs"), generated)?;
    let retained = store.join("retained");
    let mut rows = retain(upstream, &project.git_revision, &retained, &frame_hash)?;
    let cargo = rows
        .iter()
        .find(|r| r.path == "Cargo.lock")
        .ok_or("missing Cargo lock")?
        .hash
        .clone();
    let mut body = header(&project, 1)?;
    frame.write_pins(&mut body)?;
    field(&mut body, "cargoLockSha256", &cargo)?;
    for row in &rows {
        row.emit(&mut body, false)?;
    }
    verify_plan(&examples, "assembly.sha256", &body)?;
    if let Some(facade) = facade {
        require(
            facade.pins() == read(&examples.join("facade-release.json"))?,
            "facade package differs from the published lock",
        )?;
        facade.import_into(&store)?;
        let dir = facade.projection_dir(&store);
        copy_tree(facade.projected, &dir)?;
        let hash = sha(&dir.join("lib.rs"))?;
        let manifest = fs::read(dir.join("projection.json"))?;
        require(
            digest(&manifest) == facade.projection
                && serde_json::from_slice::<Value>(&manifest)?
                    == json!({"version":1,"graphSha256":facade.graph,"files":{"lib.rs":hash}}),
            "facade module differs from its projection",
        )?;
        let replaced = rows
            .iter_mut()
            .find(|r| r.path == FACADE)
            .ok_or("missing generated facade destination")?;
        require(
            matches!(replaced.producer, ProducerKind::Retained) && replaced.hash != hash,
            "expected a retained source to replace",
        )?;
        replaced.producer = ProducerKind::Facade;
        let original = std::mem::replace(&mut replaced.hash, hash);
        if !rows
            .iter()
            .any(|row| matches!(row.producer, ProducerKind::Retained) && row.hash == original)
        {
            fs::remove_file(retained.join(original))?;
        }
        body = header(&project, 2)?;
        field(&mut body, "cargoLockSha256", &cargo)?;
        for (profile, producer) in [("framing-v1", &frame), ("rust-facade-v1", &facade)] {
            body.push_str("\n[[emitters]]\n");
            field(&mut body, "profile", profile)?;
            field(&mut body, "package", "marmish-core")?;
            producer.write_pins(&mut body)?;
        }
        for row in &rows {
            row.emit(&mut body, true)?;
        }
        verify_plan(&examples, "assembly-v2.sha256", &body)?;
    }
    fs::write(stage.path().join("project.toml"), body)?;
    fs::rename(stage.path(), out)?;
    Ok(())
}
fn main() -> Result {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    match args.as_slice(){
        [version,upstream,import,projection,out] if version=="v1"=>materialize(upstream.as_ref(),Producer::load(import.as_ref(),projection.as_ref())?,None,out.as_ref()),
        [version,upstream,import,projection,facade,facade_projection,out] if version=="v2"=>materialize(upstream.as_ref(),Producer::load(import.as_ref(),projection.as_ref())?,Some(Producer::load(facade.as_ref(),facade_projection.as_ref())?),out.as_ref()),
        _=>Err("expected v1 UPSTREAM FRAME_IMPORT FRAME_PROJECTION OUTPUT or v2 UPSTREAM FRAME_IMPORT FRAME_PROJECTION FACADE_IMPORT FACADE_PROJECTION OUTPUT".into())
    }
}
