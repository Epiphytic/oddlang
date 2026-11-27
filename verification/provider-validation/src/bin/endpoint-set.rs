//! Exercise the endpoint policy generator through its public CLI and an unrelated Cargo consumer.
use provider_validation::{Result, cli, command, read, write};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

struct Witness<'a> {
    cli: &'a Path,
    work: &'a Path,
}

impl Witness<'_> {
    fn digest(&self, kind: &str, input: &str, error: Option<&str>) -> Result<String> {
        let bytes = command(
            &mut Command::new(self.cli),
            self.work,
            &["endpoint-set", kind, input],
            error,
        )?;
        Ok(String::from_utf8(bytes)?.trim().to_string())
    }

    fn build(
        &self,
        graph: &str,
        graph_pin: &str,
        overlay: Option<(&str, &str)>,
        output: &str,
        error: Option<&str>,
    ) -> Result {
        let mut args = vec!["build", "endpoint-set", "rust", graph];
        if let Some((overlay, _)) = overlay {
            args.push(overlay);
        }
        args.extend([output, "--graph-sha256", graph_pin]);
        if let Some((_, pin)) = overlay {
            args.extend(["--overlay-sha256", pin]);
        }
        command(&mut Command::new(self.cli), self.work, &args, error)?;
        if self.work.join(output).try_exists()? == error.is_some() {
            return Err(format!("unexpected output after build of {output}").into());
        }
        Ok(())
    }

    fn reject_graph(
        &self,
        original: &Value,
        change: impl FnOnce(&mut Value),
        diagnostic: &str,
    ) -> Result {
        let mut graph = original.clone();
        change(&mut graph);
        write(&self.work.join("bad.json"), &graph)?;
        self.digest("digest", "bad.json", Some(diagnostic))?;
        Ok(())
    }
}

fn main() -> Result {
    let binary = cli()?;
    let temp = tempfile::tempdir()?;
    let work = temp.path();
    let witness = Witness { cli: &binary, work };
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../endpoint-set/fixture");
    for file in ["policy.json", "rust.json"] {
        fs::copy(fixture.join(file), work.join(file))?;
    }
    let graph = read(&work.join("policy.json"))?;
    let overlay = read(&work.join("rust.json"))?;
    let graph_pin = witness.digest("digest", "policy.json", None)?;
    let overlay_pin = witness.digest("overlay-digest", "rust.json", None)?;
    witness.build(
        "policy.json",
        &graph_pin,
        Some(("rust.json", &overlay_pin)),
        "bound",
        None,
    )?;

    let mut reordered = graph.clone();
    for path in ["/errors", "/rules/5/ip/loopback", "/rules/4/forbid"] {
        reordered
            .pointer_mut(path)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| format!("missing list at {path}"))?
            .reverse();
    }
    write(&work.join("reordered.json"), &reordered)?;
    let mut reordered = overlay.clone();
    reordered["derives"]
        .as_array_mut()
        .ok_or("missing derives")?
        .reverse();
    write(&work.join("reordered-rust.json"), &reordered)?;
    assert_eq!(witness.digest("digest", "reordered.json", None)?, graph_pin);
    assert_eq!(
        witness.digest("overlay-digest", "reordered-rust.json", None)?,
        overlay_pin
    );
    witness.build(
        "reordered.json",
        &graph_pin,
        Some(("reordered-rust.json", &overlay_pin)),
        "repeat",
        None,
    )?;
    for name in ["endpoint_set.rs", "projection.lock"] {
        assert_eq!(
            fs::read(work.join("bound").join(name))?,
            fs::read(work.join("repeat").join(name))?,
            "{name}"
        );
    }
    witness.build("policy.json", &graph_pin, None, "free", None)?;
    let mut changed = graph.clone();
    changed["empty"] = json!({"kind":"allow"});
    changed["deduplicate"] = json!(false);
    changed.as_object_mut().unwrap().remove("providerHint");
    changed["rules"].as_array_mut().unwrap().swap(0, 1);
    changed["rules"][5]["dns"]["trailingDot"] = json!("preserve");
    write(&work.join("different.json"), &changed)?;
    let changed_pin = witness.digest("digest", "different.json", None)?;
    assert_ne!(changed_pin, graph_pin);
    witness.build("different.json", &changed_pin, None, "different", None)?;

    witness.build(
        "policy.json",
        &"0".repeat(64),
        None,
        "wrong",
        Some("graph digest mismatch"),
    )?;
    witness.reject_graph(
        &graph,
        |g| g["rules"].as_array_mut().unwrap().swap(0, 2),
        "input rule must precede parse",
    )?;
    witness.reject_graph(
        &graph,
        |g| g["rules"][5]["ip"]["ipv4"]["allow"] = json!(["198.51.100.42/24"]),
        "host bits",
    )?;
    witness.reject_graph(
        &graph,
        |g| g["rules"][0]["error"] = json!("urn:download-mirrors:unknown"),
        "undeclared error",
    )?;
    witness.reject_graph(
        &graph,
        |g| {
            g["rules"][3]["schemes"]
                .as_array_mut()
                .unwrap()
                .push(json!("git"))
        },
        "host-policy requires a preceding allowed-schemes",
    )?;
    witness.reject_graph(
        &graph,
        |g| {
            g["rules"].as_array_mut().unwrap().pop();
        },
        "loopback classification requires loopback-opt-in",
    )?;
    witness.reject_graph(
        &graph,
        |g| g["empty"] = json!({"kind":"allow","error":"urn:download-mirrors:empty"}),
        "unknown field",
    )?;
    let mut incomplete = overlay.clone();
    incomplete["errorVariants"]
        .as_object_mut()
        .unwrap()
        .remove("urn:download-mirrors:empty");
    write(&work.join("incomplete.json"), &incomplete)?;
    let incomplete_pin = witness.digest("overlay-digest", "incomplete.json", None)?;
    witness.build(
        "policy.json",
        &graph_pin,
        Some(("incomplete.json", &incomplete_pin)),
        "incomplete",
        Some("error mapping must cover exactly"),
    )?;
    incomplete["typeName"] = json!("Bad; unsafe");
    write(&work.join("bad-ident.json"), &incomplete)?;
    witness.digest(
        "overlay-digest",
        "bad-ident.json",
        Some("invalid Rust identifier"),
    )?;

    let consumer = work.join("consumer");
    fs::create_dir_all(consumer.join("src"))?;
    for file in ["Cargo.lock", "src/main.rs", "policy.json", "rust.json"] {
        fs::copy(fixture.join(file), consumer.join(file))?;
    }
    for (source, module) in [
        ("bound", "projection"),
        ("free", "free"),
        ("different", "different"),
    ] {
        fs::copy(
            work.join(source).join("endpoint_set.rs"),
            consumer.join("src").join(format!("{module}.rs")),
        )?;
    }
    let manifest = fs::read_to_string(fixture.join("Cargo.toml"))?;
    assert_eq!(manifest.matches("edition = \"2024\"").count(), 1);
    for edition in ["2021", "2024"] {
        fs::write(
            consumer.join("Cargo.toml"),
            manifest.replace("edition = \"2024\"", &format!("edition = \"{edition}\"")),
        )?;
        let stdout = command(
            &mut Command::new("cargo"),
            &consumer,
            &[
                "+1.97.1",
                "run",
                "--locked",
                "--offline",
                "--quiet",
                "--target-dir",
                "../consumer-target",
            ],
            None,
        )?;
        use std::io::Write;
        std::io::stdout().write_all(&stdout)?;
    }
    println!("endpoint-set qualification passed in Rust editions 2021 and 2024");
    Ok(())
}
