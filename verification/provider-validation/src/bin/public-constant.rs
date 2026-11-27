//! Replay the observatory source, rustdoc, projection and independent native caller.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run(command: &mut Command, code: i32) -> Result<String> {
    let output = command.output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.code() != Some(code) {
        return Err(format!("{command:?}: {} {text}", output.status).into());
    }
    Ok(text)
}
fn digest(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

struct Observation<'a> {
    root: &'a Path,
    cli: &'a Path,
    source: PathBuf,
    doc: PathBuf,
}
impl<'a> Observation<'a> {
    fn new(root: &'a Path, cli: &'a Path, text: &str) -> Result<Self> {
        fs::create_dir(root)?;
        let source = root.join("observatory.rs");
        fs::write(&source, text)?;
        run(
            Command::new("rustdoc")
                .args([
                    "+1.97.1",
                    "-Z",
                    "unstable-options",
                    "--output-format",
                    "json",
                    "--crate-name",
                    "observatory",
                    "observatory.rs",
                ])
                .env("RUSTC_BOOTSTRAP", "1")
                .current_dir(root),
            0,
        )?;
        Ok(Self {
            root,
            cli,
            source,
            doc: root.join("doc/observatory.json"),
        })
    }

    fn select(&self, name: &str, dir: &Path, doc: &Path, error: Option<&str>) -> Result<()> {
        let selection = self.root.join("selection.json");
        fs::write(
            &selection,
            serde_json::to_vec(&json!({
                "format":"oddlang-public-constant-selection-v1","id":"urn:observatory:limits",
                "constantId":format!("urn:observatory:limits:{name}"),"crateName":"observatory","name":name,
                "sourceFilename":"observatory.rs","sourceSha256":digest(&self.source)?,"rustdocSha256":digest(doc)?
            }))?,
        )?;
        let output = run(
            Command::new(self.cli)
                .args(["recover", "public-constant"])
                .arg(&self.source)
                .arg(doc)
                .arg(selection)
                .arg(dir),
            i32::from(error.is_some()),
        )?;
        if let Some(error) = error {
            if !output.contains(error) || dir.exists() {
                return Err(format!("unexpected rejected projection: {output}").into());
            }
        }
        Ok(())
    }

    fn compare(&self, name: &str, ty: &str, value: u16, expression: Value) -> Result<Value> {
        let out = self.root.join(name);
        let repeat = self.root.join(format!("{name}-repeat"));
        for dir in [&out, &repeat] {
            self.select(name, dir, &self.doc, None)?;
        }
        for file in ["graph.json", "rust.json", "recovery.lock"] {
            assert_eq!(fs::read(out.join(file))?, fs::read(repeat.join(file))?);
        }
        let graph: Value = serde_json::from_slice(&fs::read(out.join("graph.json"))?)?;
        assert_eq!(
            graph["constants"],
            json!([{"id":format!("urn:observatory:limits:{name}"),"value":expression}])
        );
        let lock: Value = serde_json::from_slice(&fs::read(out.join("recovery.lock"))?)?;
        assert_eq!(lock["unsignedDecimal"], value.to_string());
        assert_eq!(lock["graphSha256"], digest(&out.join("graph.json"))?);
        let projected = self.root.join(format!("projection-{name}"));
        run(
            Command::new(self.cli)
                .args(["build", "shared-declarations", "rust"])
                .arg(out.join("graph.json"))
                .arg(out.join("rust.json"))
                .arg(&projected),
            0,
        )?;
        let caller = self.root.join("caller.rs");
        fs::write(
            &caller,
            format!(
                "const VALUE: {ty} = observatory::{name};\nconst _: [(); {value}] = [(); VALUE as usize];\nfn main() {{ println!(\"{{}}\",VALUE); }}\n"
            ),
        )?;
        for file in [&self.source, &projected.join("shared.rs")] {
            let lib = self.root.join("libobservatory.rlib");
            run(
                Command::new("rustc")
                    .args([
                        "+1.97.1",
                        "--edition",
                        "2021",
                        "--crate-name",
                        "observatory",
                        "--crate-type",
                        "rlib",
                    ])
                    .arg(file)
                    .arg("-o")
                    .arg(&lib),
                0,
            )?;
            let binary = self.root.join("caller");
            run(
                Command::new("rustc")
                    .args(["+1.97.1", "--edition", "2021"])
                    .arg(&caller)
                    .arg("--extern")
                    .arg(format!("observatory={}", lib.display()))
                    .arg("-o")
                    .arg(&binary),
                0,
            )?;
            assert_eq!(run(&mut Command::new(binary), 0)?, format!("{value}\n"));
        }
        Ok(graph)
    }
}

fn main() -> Result<()> {
    let cli = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or("usage: public-constant ODDLANG")?,
    )
    .canonicalize()?;
    let dir = tempfile::tempdir()?;
    let source = include_str!("../../../public-constant/observatory.rs");
    let a = dir.path().join("baseline");
    let b = dir.path().join("mutation");
    let baseline = Observation::new(&a, &cli, source)?;
    let mutated = Observation::new(&b, &cli, &source.replace(" * 7;", " * 8;"))?;
    let expression = |factor| json!({"multiply":[{"multiply":[3,5]},factor]});
    let original = baseline.compare("GRID_LIMIT", "u16", 105, expression(7))?;
    baseline.compare("SAMPLE_COUNT", "usize", 42, json!(42))?;
    assert_ne!(
        original,
        mutated.compare("GRID_LIMIT", "u16", 120, expression(8))?
    );
    for observation in [&baseline, &mutated] {
        observation.select(
            "PRIVATE_LIMIT",
            &observation.root.join("private"),
            &observation.doc,
            Some("expected exactly one selected local root child"),
        )?;
    }
    mutated.select(
        "GRID_LIMIT",
        &b.join("stale"),
        &baseline.doc,
        Some("disagrees with compiler evaluation"),
    )?;
    println!(
        "Public constant recovery, repeated generation, native consumers and negative cases passed."
    );
    Ok(())
}
