use super::*;
use std::collections::BTreeMap;

fn package(s: &Suite, graph: &Path) -> Result {
    let inputs = s.root.join("package-inputs");
    fs::create_dir(&inputs)?;
    fs::copy(graph, inputs.join("graph.json"))?;
    for file in ["binding.json", "Cargo.lock"] {
        fs::copy(s.fixture.join(file), inputs.join(file))?;
    }
    let mut manifest = json!({"format":"oddlang-native-package-v1","name":"observatory","edition":"2021","version":"2.1.0",
        "cargoLock":pin(&inputs,"Cargo.lock")?,"dependencies":{"clap":{"version":"4.6.7","features":["derive"]}},
        "components":[{"kind":"cli-surface","module":"command","graph":pin(&inputs,"graph.json")?,"overlay":pin(&inputs,"binding.json")?,
        "reexports":["run_cli","run_cli_with"]}]});
    let file = inputs.join("package.json");
    let build = |dir: &Path, error| {
        s.expect(
            Command::new(&s.cli)
                .args(["build", "native-package", "rust"])
                .arg(&file)
                .arg(dir),
            error,
        )
    };
    write(&file, &manifest)?;
    for name in ["package-first", "package-second"] {
        let dir = s.root.join(name);
        build(&dir, None)?;
        s.cargo(&dir, "check")?;
    }
    let lock = fs::read(s.root.join("package-first/package.lock"))?;
    assert_eq!(lock, fs::read(s.root.join("package-second/package.lock"))?);
    assert!(String::from_utf8(lock)?.contains("oddlang-command-surface-lock-v1"));
    manifest["dependencies"]["clap"]["version"] = json!("4.6.6");
    write(&file, &manifest)?;
    build(
        &s.root.join("package-invalid"),
        Some("pinned clap 4.6.7 provider"),
    )
}
fn source_controls(s: &Suite) -> Result {
    let file = s.root.join("unsupported.rs");
    let output = s.root.join("unsupported.json");
    let selection = s.fixture.join("selection.json");
    let source = fs::read_to_string(s.fixture.join("src/bin/reference.rs"))?;
    fs::write(
        &file,
        source.replace("visible_alias = \"scan\"", "long_about = \"unsupported\""),
    )?;
    s.recover("cli-surface", &file, &selection, &output, Some(""))?;
    assert!(!output.exists());
    let added = source.replace(
        "    sensor: OsString,",
        "    sensor: OsString,\n    #[arg(long, value_enum)]\n    filter: Option<Filter>,",
    ) + r#"
#[derive(clap::ValueEnum, Clone)]
enum Filter {
    /// Use the red filter.
    Red,
}"#;
    fs::write(&file, &added)?;
    s.recover(
        "cli-surface",
        &file,
        &selection,
        &output,
        Some("unsupported value variant Red"),
    )?;
    fs::write(
        &file,
        added.replace("    /// Use the red filter.\n", "").replace(
            "enum Filter {",
            "#[value(rename_all = \"UPPER\")]\nenum Filter {",
        ),
    )?;
    s.recover(
        "cli-surface",
        &file,
        &selection,
        &output,
        Some("unsupported value enum settings"),
    )
}
fn mutations(s: &Suite, original: &Value, generated: &Path, exe: &Path) -> Result {
    let file = s.root.join("mutated.json");
    let binding = s.fixture.join("binding.json");
    let rejected = s.root.join("rejected.rs");
    let cli = generated.join("src/cli.rs");
    let mut graph = original.clone();
    let count = "/root/subcommands/0/arguments/0";
    graph.pointer_mut(count).unwrap()["bounds"]["maximum"] = json!(9);
    write(&file, &graph)?;
    s.project(&file, &binding, &cli, None)?;
    s.cargo(generated, "build")?;
    let bad = s.observe(exe, &["capture", "--count", "10", "beta"])?;
    assert_eq!(bad.status, 12);
    assert!(bad.stderr_has("1..=9"));
    graph.pointer_mut(count).unwrap()["bounds"]["maximum"] = json!(0);
    write(&file, &graph)?;
    s.project(&file, &binding, &rejected, Some(""))?;
    assert!(!rejected.exists());
    graph.pointer_mut(count).unwrap()["bounds"]["maximum"] = json!(10);
    graph.pointer_mut(count).unwrap()["default_value"] = json!("not-an-integer");
    write(&file, &graph)?;
    s.project(&file, &binding, &rejected, Some(""))?;
    assert!(!rejected.exists());
    graph.pointer_mut(count).unwrap()["default_value"] = json!("3");
    let args = graph["root"]["subcommands"][0]["arguments"]
        .as_array_mut()
        .unwrap();
    args.last_mut().unwrap()["repeated"] = json!(true);
    args.push(json!({"id":"tail","kind":"string","value_name":"TAIL"}));
    write(&file, &graph)?;
    s.project(&file, &binding, &rejected, Some("ambiguous placement"))?;
    source_controls(s)?;
    let mut graph = original.clone();
    graph["root"]["arguments"][0]["conflicts"] = json!(["quiet"]);
    graph["root"]["arguments"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"quiet","long":"quiet","value_name":"QUIET"}));
    write(&file, &graph)?;
    s.project(
        &file,
        &binding,
        &rejected,
        Some("invalid argument relationship for site"),
    )?;
    let mut external = original.clone();
    external["external"] = json!(true);
    external["root"]["subcommands"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"remote","subcommands":[{"name":"add"}]}));
    write(&file, &external)?;
    let mut local = read(&binding)?;
    local["operations"]["remote/add"] = json!("urn:observatory:remote:add");
    local["operations"]["*"] = json!("urn:observatory:external");
    let overlay = s.root.join("external.json");
    write(&overlay, &local)?;
    s.project(&file, &overlay, &cli, None)?;
    fs::write(
        generated.join("src/main.rs"),
        r#"
mod cli;
fn main() -> std::process::ExitCode {
    let _: fn() -> std::process::ExitCode = cli::run_cli;
    cli::run_cli_with(|operation, _| {
        println!("{operation}");
        std::process::ExitCode::SUCCESS
    })
}
"#,
    )?;
    s.cargo(generated, "build")?;
    s.observe(exe, &["remote", "add"])?
        .exact(0, b"urn:observatory:remote:add\n", b"");
    s.observe(exe, &["remote/add"])?
        .exact(0, b"urn:observatory:external\n", b"");
    Ok(())
}
pub(super) fn verify(s: &Suite) -> Result {
    let graph = s.root.join("graph.json");
    let repeat = s.root.join("again.json");
    s.base(&graph)?;
    s.base(&repeat)?;
    assert_eq!(fs::read(&graph)?, fs::read(repeat)?);
    let reference = s.root.join("reference");
    let generated = s.root.join("generated");
    for dir in [&reference, &generated] {
        s.scaffold(dir)?;
    }
    fs::copy(
        s.fixture.join("src/bin/reference.rs"),
        reference.join("src/main.rs"),
    )?;
    fs::copy(
        s.fixture.join("generated.rs"),
        generated.join("src/main.rs"),
    )?;
    s.project(
        &graph,
        &s.fixture.join("binding.json"),
        &generated.join("src/cli.rs"),
        None,
    )?;
    s.cargo(&reference, "build")?;
    s.cargo(&generated, "build")?;
    let exe = generated.join("target/debug/observatory");
    let oracle = reference.join("target/debug/observatory");
    let mut observed = BTreeMap::new();
    for case in s.cases("check")? {
        let args: Vec<_> = case.iter().map(String::as_str).collect();
        let out = s.compare(&oracle, &exe, &args)?;
        observed.insert(case.join(" "), out);
    }
    let help = &observed["--help"];
    assert!(help.status == 0 && help.stdout_has("Usage: telescope") && help.stderr.is_empty());
    observed["--version"].exact(0, b"observatory 2.1.0\n", b"");
    let absent = &observed["--absent"];
    assert!(
        absent.status == 12
            && absent.stdout.is_empty()
            && absent.stderr.ends_with(b"telescope: invalid arguments\n")
    );
    observed["calibrate"].exact(125, b"", b"command operation unavailable\n");
    observed["capture alpha"].exact(0, b"urn:observatory:capture summit 3 alpha\n", b"");
    package(s, &graph)?;
    mutations(s, &read(&graph)?, &generated, &exe)
}
