use provider_validation::{Result, cargo, command, read, sha, write};
use serde::Deserialize;
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

mod field_conjunction;
mod frame_fragments;
mod frame_guards;
mod tagged_frame;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    edits: Vec<Edit>,
    expected: Option<Vec<String>>,
    unresolved: Option<String>,
    variant: Option<String>,
}
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Edit {
    Replace { before: String, after: String },
    Append { append: String },
}
impl Case {
    fn revision(&self, base: &str) -> Result<String> {
        let mut text = base.to_owned();
        for edit in &self.edits {
            match edit {
                Edit::Append { append } => text.push_str(append),
                Edit::Replace { before, after } => {
                    if text.matches(before).count() != 1 {
                        return Err(
                            format!("{}: expected one match for {before:?}", self.name).into()
                        );
                    }
                    text = text.replacen(before, after, 1);
                }
            }
        }
        Ok(text)
    }
}
struct Suite {
    work: PathBuf,
    project: PathBuf,
    source: PathBuf,
    fixtures: PathBuf,
    cli: PathBuf,
    package: &'static str,
    option: &'static str,
    method: &'static str,
    host: String,
}
impl Suite {
    fn observe(&self, evidence: &Path) -> Result<Value> {
        let mut cmd = provider_validation::capture_command();
        cmd.arg(self.project.join("Cargo.toml"))
            .env(self.option, self.method);
        let raw = command(
            &mut cmd,
            &self.project,
            &[self.package, "lib", self.package, &self.host, "default"],
            None,
        )?;
        fs::write(evidence, &raw)?;
        Ok(serde_json::from_slice(&raw)?)
    }
    fn recover(
        &self,
        route: &str,
        template: &Path,
        overlay: &Path,
        out: &Path,
        error: Option<&str>,
    ) -> Result {
        let mut cmd = Command::new(&self.cli);
        cmd.args(["recover", route])
            .arg(&self.project)
            .arg(&self.source)
            .arg(self.work.join("compiler.json"))
            .arg(self.work.join("selection.json"))
            .arg(template)
            .arg(overlay)
            .arg(out);
        command(&mut cmd, &self.work, &[], error)?;
        if out.exists() == error.is_some() {
            return Err(format!("unexpected output state: {}", out.display()).into());
        }
        Ok(())
    }
    fn select(&self, mut selection: Value) -> Result {
        selection["sourceSha256"] = sha(&self.source)?.into();
        selection["evidenceSha256"] = sha(&self.work.join("compiler.json"))?.into();
        write(&self.work.join("selection.json"), &selection)
    }
    fn build(&self, route: &str, graph: &Path, overlay: &Path, out: &Path) -> Result<String> {
        let mut cmd = Command::new(&self.cli);
        cmd.args(["build", route, "rust"])
            .arg(graph)
            .arg(overlay)
            .arg(out);
        command(&mut cmd, &self.work, &[], None)?;
        let mut sources = Vec::new();
        for entry in fs::read_dir(out)? {
            let p = entry?.path();
            if p.extension().is_some_and(|ext| ext == "rs") {
                sources.push(p);
            }
        }
        let [source] = sources.as_slice() else {
            return Err(format!("expected one Rust projection in {}", out.display()).into());
        };
        Ok(fs::read_to_string(source)?)
    }
    fn native(&self, text: &str, expected: &[String]) -> Result {
        let caller = fs::read_to_string(self.fixtures.join("caller.rs"))?;
        fs::write(self.work.join("witness.rs"), format!("{text}{caller}"))?;
        command(
            &mut Command::new("rustc"),
            &self.work,
            &["--edition=2024", "witness.rs", "-o", "witness"],
            None,
        )?;
        let out = command(
            &mut Command::new(self.work.join("witness")),
            &self.work,
            &[],
            None,
        )?;
        assert_eq!(
            String::from_utf8(out)?,
            format!("{}\n", expected.join("\n"))
        );
        Ok(())
    }
}
fn unresolved(value: &Value, needle: Option<&str>) {
    let reasons = value["unresolved"]
        .as_array()
        .expect("unresolved observations");
    assert!(!reasons.is_empty());
    if let Some(needle) = needle {
        assert!(
            reasons
                .iter()
                .any(|msg| msg.as_str().unwrap().contains(needle)),
            "{value}"
        );
    }
}
fn resolved(value: &Value) {
    assert_eq!(value["unresolved"], serde_json::json!([]), "{value}");
}

fn main() -> Result {
    let args: Vec<_> = env::args().skip(1).collect();
    let [name, cli] = args.as_slice() else {
        return Err("expected SUITE ODDLANG".into());
    };
    let (package, option, method, verify): (_, _, _, fn(&Suite, &[Case], &str) -> Result) =
        match name.as_str() {
            "frame-guards" => (
                "parcel",
                "ODD_RECOVERY_TAGGED_FRAME_GUARDS",
                "crate::Parcel::encode#Chunk",
                frame_guards::verify,
            ),
            "frame-fragments" => (
                "parts",
                "ODD_RECOVERY_TAGGED_FRAME_FRAGMENTS",
                "crate::Envelope::encode#Head,Copy",
                frame_fragments::verify,
            ),
            "field-conjunction" => (
                "fixture",
                "ODD_RECOVERY_FIELD_CONJUNCTION",
                "crate::Invoice::agrees",
                field_conjunction::verify,
            ),
            "tagged-frame" => (
                "packet",
                "ODD_RECOVERY_TAGGED_FRAME",
                "crate::Packet::encode",
                tagged_frame::verify,
            ),
            _ => return Err("unknown recovery suite".into()),
        };
    let scratch = tempfile::tempdir()?;
    let work = scratch.path().canonicalize()?;
    let project = work.join("fixture");
    fs::create_dir_all(project.join("src"))?;
    let rustc = String::from_utf8(command(&mut Command::new("rustc"), &work, &["-vV"], None)?)?;
    let host = rustc
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("missing compiler host")?
        .to_owned();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rust-recovery")
        .join(name);
    let original = fs::read_to_string(fixtures.join("fixture.rs"))?;
    let suite = Suite {
        work,
        source: project.join("src/lib.rs"),
        project,
        cli: Path::new(cli).canonicalize()?,
        fixtures,
        package,
        host,
        option,
        method,
    };
    fs::write(
        suite.project.join("Cargo.toml"),
        format!("[package]\nname=\"{package}\"\nversion=\"0.1.0\"\nedition=\"2024\"\n"),
    )?;
    fs::write(&suite.source, &original)?;
    cargo(&suite.project, &["generate-lockfile", "--offline"])?;
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(suite.fixtures.join("cases.json"))?)?;
    verify(&suite, &cases, &original)?;
    println!(
        "{name}: {} compiler observations and independent native caller comparisons passed",
        cases.len()
    );
    Ok(())
}
