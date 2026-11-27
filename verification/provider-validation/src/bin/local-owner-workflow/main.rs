//! Check the generated field-meter workflow with a fixed, private Unix socket peer.
mod peer;
use clap::{Parser, ValueEnum};
use provider_validation::{Result, pin, read, sha, subprocess::run, write};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Copy, Clone, ValueEnum)]
enum Mode {
    Workflow,
    Delivery,
    Deferred,
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Delivery => "delivery",
            Self::Deferred => "deferred",
        }
    }
}
#[derive(Parser)]
struct Args {
    #[arg(value_enum)]
    mode: Mode,
    oddlang: PathBuf,
}

fn checked(command: &mut Command) -> Result {
    let result = run(command, None)?;
    if !result.status.success() {
        return Err(format!(
            "{command:?}: {}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}
fn project(cli: &Path, graph: &Path, overlay: &Path, out: &Path) -> Command {
    let mut cmd = Command::new(cli);
    cmd.args(["build", "local-command-client", "rust"])
        .arg(graph)
        .arg(overlay)
        .arg(out);
    cmd
}
fn build(dir: &Path) -> Result<PathBuf> {
    let target = dir.join("target");
    checked(
        Command::new("cargo")
            .args(["+1.97.1", "build", "--locked", "--offline", "--release"])
            .env("CARGO_TARGET_DIR", &target)
            .current_dir(dir),
    )?;
    Ok(target.join("release/field-meter"))
}
fn native_package(cli: &Path, root: &Path, command_fixture: &Path) -> Result<PathBuf> {
    for file in ["Cargo.lock", "cli.json"] {
        fs::copy(command_fixture.join(file), root.join(file))?;
    }
    write(
        &root.join("cli-overlay.json"),
        &json!({"executable":"field-meter","invalid_exit":2,"uncovered_exit":125,
        "operations":{"read":"urn:field:read"},"handlers":{"urn:field:read":["meter","measure"]}}),
    )?;
    let manifest = json!({"format":"oddlang-native-package-v1","name":"field-meter","version":"1.0.0","edition":"2021","dependencies":{
        "clap":{"version":"4.6.7"},"libc":{"version":"0.2.189"},"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.151"},"tokio":{"version":"1.54.1","features":["io-util","net","rt","time"]}},
        "cargoLock":pin(root,"Cargo.lock")?,"components":[{"kind":"cli-surface","module":"cli","graph":pin(root,"cli.json")?,"overlay":pin(root,"cli-overlay.json")?,"reexports":["run_cli"]},
        {"kind":"local-command-client","module":"meter","graph":pin(root,"graph.json")?,"overlay":pin(root,"rust.json")?,"reexports":["measure"],"rootReexports":[]}],"binaries":[{"name":"field-meter","entry":["run_cli"]}]});
    let manifest_path = root.join("manifest.json");
    write(&manifest_path, &manifest)?;
    let product = root.join("native");
    checked(
        Command::new(cli)
            .args(["build", "native-package", "rust"])
            .arg(manifest_path)
            .arg(&product),
    )?;
    build(&product)
}
fn delivery_binary(root: &Path, fixture: &Path, projection: &Path) -> Result<PathBuf> {
    let mut sources = vec![];
    for item in fs::read_dir(projection)? {
        let p = item?.path();
        if p.extension().is_some_and(|e| e == "rs") {
            sources.push(p)
        }
    }
    let [source] = sources.as_slice() else {
        return Err("expected one generated meter module".into());
    };
    fs::create_dir(root.join("src"))?;
    fs::copy(source, root.join("src/generated.rs"))?;
    fs::copy(fixture.join("main.rs"), root.join("src/main.rs"))?;
    fs::copy(fixture.join("Cargo.toml"), root.join("Cargo.toml"))?;
    checked(
        Command::new("cargo")
            .args(["+1.97.1", "generate-lockfile", "--offline"])
            .current_dir(root),
    )?;
    build(root)
}
fn reject(cli: &Path, root: &Path, graph: &Value, error: &str) -> Result {
    let file = root.join("invalid-workflow.json");
    write(&file, graph)?;
    let out = root.join("invalid");
    let result = run(
        &mut project(cli, &file, &root.join("rust.json"), &out),
        None,
    )?;
    if result.status.success()
        || !String::from_utf8_lossy(&result.stderr).contains(error)
        || out.try_exists()?
    {
        return Err(format!("expected unpublished graph with {error}, got {result:?}").into());
    }
    Ok(())
}
fn mutations(cli: &Path, root: &Path, graph: &Value) -> Result {
    let mut changed = graph.clone();
    changed["steps"][1]["transitions"].as_array_mut().ok_or("step transitions")?.push(json!({
        "id":"urn:meter:workflow:transition:stale","when":[{"path":["result"],"equals":"already"}],"capture":[],
        "effect":{"kind":"continue","step":"urn:meter:workflow:step:sample","delay_ms":0}}));
    reject(
        cli,
        root,
        &changed,
        "cannot carry workflow capture slot before assignment",
    )?;
    let mut changed = graph.clone();
    changed["steps"][2]["transitions"][0]["carry"] = json!(["ghost"]);
    reject(cli, root, &changed, "undeclared workflow capture ghost")?;
    let mut changed = graph.clone();
    changed["steps"][0]["transitions"][0]["when"][0]["equals"] = json!({});
    reject(cli, root, &changed, "invalid workflow reply predicate")?;
    let mut changed = graph.clone();
    changed["transport"]["maximum_frame_bytes"] = json!(2);
    reject(
        cli,
        root,
        &changed,
        "minimum workflow request exceeds frame limit",
    )
}
fn main() -> Result {
    let args = Args::parse();
    let cli = args.oddlang.canonicalize()?;
    let verification = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let fixtures = verification.join("local-owner-workflow");
    let mode = args.mode;
    let selected = fixtures.join(mode.name());
    let temp = tempfile::Builder::new().prefix("odw-").tempdir_in("/tmp")?;
    let root = temp.path().canonicalize()?;
    let input = root.join("graph.json");
    fs::copy(selected.join("graph.json"), &input)?;
    let graph = read(&input)?;
    let overlay = root.join("rust.json");
    let mut binding = json!({"format":"oddlang-local-command-client-rust-v1","entry":"measure"});
    if !matches!(mode, Mode::Workflow) {
        binding["delivery_provider"] = json!("base64-sha256");
    }
    write(&overlay, &binding)?;
    let projection = root.join("projection");
    checked(&mut project(&cli, &input, &overlay, &projection))?;
    let binary = match mode {
        Mode::Workflow => native_package(&cli, &root, &verification.join("local-command-client"))?,
        _ => delivery_binary(&root, &fixtures.join("delivery"), &projection)?,
    };
    let count = peer::verify(&selected.join("cases.json"), &binary, &root.join("sock"))?;
    let obligations = projection.join("workflow-obligations.json");
    let digest = sha(&obligations)?;
    let lock = read(&projection.join("projection.lock"))?;
    assert_eq!(lock["workflowObligationsSha256"], digest);
    let catalog = read(&obligations)?;
    match mode {
        Mode::Workflow => {
            assert_eq!(count, 10);
            assert_eq!(
                catalog["captureLifetime"],
                "nextStepUnlessExplicitlyCarried"
            );
            assert_eq!(
                catalog["obligations"][1]["request"][1]["representation"],
                json!({"type":"string","absent":"reject"})
            );
            mutations(&cli, &root, &graph)?;
        }
        Mode::Delivery => {
            assert_eq!(count, 17);
            assert_eq!(
                catalog["delivery"]["cursorOrigin"],
                "declared unsigned capture or zero"
            );
        }
        Mode::Deferred => {
            assert_eq!(count, 4);
            let mut changed = graph;
            let captures = changed["steps"][1]["transitions"][0]["capture"]
                .as_array_mut()
                .ok_or("capture declarations")?;
            let retained = captures
                .iter_mut()
                .find(|c| c["name"] == "retained-reply")
                .ok_or("missing retained-reply")?;
            retained["value_kind"]["maximum_bytes"] = json!(0);
            reject(
                &cli,
                &root,
                &changed,
                "invalid workflow capture retained-reply",
            )?;
        }
    }
    println!(
        "{}: {count} independent meter cases passed; obligations SHA-256 {digest}",
        mode.name()
    );
    Ok(())
}
