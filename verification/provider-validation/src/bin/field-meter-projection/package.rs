use super::socket_cases::{Case, observe};
use provider_validation::{Result, command, invoke, pin, read, sha, write};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

struct Package<'a> {
    cli: &'a Path,
    root: &'a Path,
    manifest: Value,
}

impl Package<'_> {
    fn generate(
        &self,
        cmd: &Value,
        device: &Value,
        overlay: &Value,
        out: &str,
        expected: Option<&str>,
    ) -> Result {
        let mut manifest = self.manifest.clone();
        for (name, data) in [
            ("cli.json", cmd),
            ("graph.json", device),
            ("cli-rust.json", overlay),
        ] {
            write(&self.root.join(name), data)?;
        }
        for component in manifest["components"]
            .as_array_mut()
            .ok_or("missing components")?
        {
            for key in ["graph", "overlay"] {
                let path = component[key]["path"]
                    .as_str()
                    .ok_or("missing component path")?;
                component[key] = pin(self.root, path)?;
            }
        }
        // The independent package template pins the fixture's Cargo.lock.
        if manifest["cargoLock"] != pin(self.root, "Cargo.lock")? {
            return Err("field-meter Cargo.lock drift".into());
        }
        write(&self.root.join("manifest.json"), &manifest)?;
        invoke(
            self.cli,
            self.root,
            &["build", "native-package", "rust", "manifest.json"],
            out,
            expected,
        )
    }
    fn build(&self, cmd: &Value, device: &Value, overlay: &Value, out: &str) -> Result<PathBuf> {
        self.generate(cmd, device, overlay, out, None)?;
        let workspace = self.root.join(out);
        command(
            &mut Command::new("cargo"),
            &workspace,
            &["+1.97.1", "build", "--release", "--locked", "--offline"],
            None,
        )?;
        Ok(workspace
            .join(env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
            .join("release/field-meter"))
    }
}

fn rejected(binary: &Path, args: &[&str], code: i32, diagnostic: &str) -> Result {
    observe(Command::new(binary).args(args), code, "", diagnostic)
}
fn exchanges(cases: &[Case], root: &Path, binary: &Path) -> Result {
    for case in cases {
        case.exchange(root, &mut Command::new(binary))?;
    }
    Ok(())
}
fn discovery(binary: &Path, root: &Path, cases: &[Case]) -> Result {
    let [discovered, explicit] = cases else {
        return Err("expected discovery and explicit socket cases".into());
    };
    let home = root.join("h");
    let scratch = root.join("t");
    let state = home.join("meter");
    let station = state.join("bench");
    let runtime = scratch.join(format!("meter-{}-1a2b", fs::metadata(root)?.uid()));
    for dir in [&home, &scratch, &state, &station, &runtime] {
        fs::DirBuilder::new().mode(0o700).create(dir)?;
    }
    let profile = station.join("station.json");
    let binding = runtime.join("link.json");
    write(&profile, &json!({"slot":"1a2b"}))?;
    write(&binding, &json!({"path":station.canonicalize()?}))?;
    for file in [&profile, &binding] {
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))?;
    }
    let client = |store: &Path| {
        let mut cmd = Command::new(binary);
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", &scratch)
            .env("METER_STORE", store);
        cmd
    };
    discovered.exchange(root, &mut client(&home))?;
    write(&binding, &json!({"path":"/elsewhere"}))?;
    observe(
        client(&home).arg("read"),
        8,
        "",
        "incorrect station binding\n",
    )?;
    fs::set_permissions(profile, fs::Permissions::from_mode(0o644))?;
    observe(client(&home).arg("read"), 8, "", "unsafe station file\n")?;
    let relative = Path::new("relative");
    observe(client(relative).arg("read"), 8, "", "missing meter store\n")?;
    explicit.exchange(root, &mut client(relative))
}

pub fn verify(cli: &Path, fixture: &Path) -> Result {
    let temp = tempfile::Builder::new().prefix("fmc-").tempdir_in("/tmp")?;
    let root = temp.path();
    let cases: BTreeMap<String, Vec<Case>> =
        serde_json::from_slice(&fs::read(fixture.join("socket-cases.json"))?)?;
    let cmd = read(&fixture.join("cli.json"))?;
    let graph = read(&fixture.join("graph.json"))?;
    let overlay = read(&fixture.join("cli-rust.json"))?;
    for file in ["Cargo.lock", "binding.json"] {
        fs::copy(fixture.join(file), root.join(file))?;
    }
    let expected = [
        ("generated", 9),
        ("dynamic-client", 2),
        ("typed-client", 11),
        ("discovery-client", 2),
    ];
    if cases.len() != expected.len()
        || !expected
            .iter()
            .all(|(name, n)| cases.get(*name).is_some_and(|v| v.len() == *n))
    {
        return Err("incomplete field-meter socket cases".into());
    }
    if cases["dynamic-client"][0].request_size() != 128 {
        return Err("expected exact 128-byte request".into());
    }
    let package = Package {
        cli,
        root,
        manifest: read(&fixture.join("package.json"))?,
    };
    // Check the original fixture pins before composing the variants.
    for component in package.manifest["components"]
        .as_array()
        .ok_or("missing components")?
    {
        for key in ["graph", "overlay"] {
            let path = component[key]["path"]
                .as_str()
                .ok_or("missing fixture path")?;
            if component[key] != pin(fixture, path)? {
                return Err(format!("field-meter fixture drift: {path}").into());
            }
        }
    }
    let binary = package.build(&cmd, &graph, &overlay, "generated")?;
    exchanges(&cases["generated"], root, &binary)?;
    let socket = root.join("s");
    let path = socket.to_str().ok_or("non-UTF8 socket path")?;
    rejected(
        &binary,
        &["read", "--port", path, "--unit", "metric"],
        4,
        "invalid unit\n",
    )?;
    rejected(&binary, &["read"], 6, "unbound sensor\n")?;
    rejected(&binary, &["read", "--port", path], 7, "sensor I/O\n")?;

    let mut dynamic = graph.clone();
    dynamic["request_id"] = json!("urn:field:request");
    dynamic["request"] = read(&fixture.join("typed-request.json"))?;
    let mut arguments = cmd.clone();
    arguments["root"]["arguments"].as_array_mut().unwrap().extend([
        json!({"id":"sensor","long":"sensor","kind":"u64","global":true,"default_value":"7"}),
        json!({"id":"label","long":"label","kind":"string","global":true}),
        json!({"id":"waiting","long":"waiting","global":true}),
        json!({"id":"probe","long":"probe","kind":"string","global":true,"repeated":true,"default_value":"backup"}),
    ]);
    let binary = package.build(&arguments, &dynamic, &overlay, "dynamic-client")?;
    exchanges(&cases["dynamic-client"], root, &binary)?;
    rejected(
        &binary,
        &["read", "--port", path, "--label", "a"],
        6,
        "unbound sensor\n",
    )?;
    let mut oversized = cases["dynamic-client"][0].arguments(root)?;
    let index = oversized
        .iter()
        .position(|a| a == "--label")
        .ok_or("missing label")?
        + 1;
    oversized[index].push('x');
    observe(
        Command::new(&binary).args(oversized),
        9,
        "",
        "invalid sensor frame size\n",
    )?;
    for (id, key, value) in [
        ("probe", "repeated", json!(false)),
        ("sensor", "kind", json!("string")),
        ("waiting", "kind", json!("string")),
    ] {
        let mut invalid = arguments.clone();
        let arg = invalid["root"]["arguments"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|v| v["id"] == id)
            .unwrap();
        arg[key] = value;
        package.generate(
            &invalid,
            &dynamic,
            &overlay,
            "invalid",
            Some(&format!("incompatible argument \"{id}\"")),
        )?;
    }
    let mut typed = graph.clone();
    typed["transport"]["maximum_frame_bytes"] = json!(512);
    typed["response"] = read(&fixture.join("typed-response.json"))?;
    let binary = package.build(&cmd, &typed, &overlay, "typed-client")?;
    exchanges(&cases["typed-client"], root, &binary)?;

    let mut endpoint = graph.clone();
    endpoint["discovery"] = read(&fixture.join("discovery.json"))?;
    let mut discover_cli = cmd.clone();
    for (id, kind) in [("folder", "path"), ("station", "string")] {
        discover_cli["root"]["arguments"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"long":id,"kind":kind,"global":true}));
    }
    super::project(cli, root, &endpoint, "effects", None)?;
    let effects = root.join("effects");
    if read(&effects.join("projection.lock"))?["endpointObligationsSha256"]
        != sha(&effects.join("endpoint-obligations.json"))?
    {
        return Err("endpoint catalog digest mismatch".into());
    }
    let binary = package.build(&discover_cli, &endpoint, &overlay, "discovery-client")?;
    discovery(&binary, root, &cases["discovery-client"])?;
    let mut escape = endpoint.clone();
    escape["discovery"]["socket_leaf"] = json!("../sock");
    super::project(
        cli,
        root,
        &escape,
        "invalid",
        Some("invalid local endpoint discovery policy"),
    )?;
    endpoint["discovery"]["profile"]["checks"][0]["require"]["value"] =
        json!({"kind":"binding_field","id":"urn:meter:link:path"});
    super::project(
        cli,
        root,
        &endpoint,
        "invalid",
        Some("runtime binding field used before reading it"),
    )?;

    for (guard, expected) in [
        ("units", "needs undeclared root argument"),
        ("port", "has incompatible argument"),
    ] {
        let mut invalid = graph.clone();
        invalid["guards"][0]["argument"] = json!(guard);
        package.generate(&cmd, &invalid, &overlay, "invalid", Some(expected))?;
    }
    let mut shadowed = cmd.clone();
    shadowed["root"]["arguments"][1]["global"] = json!(false);
    let mut unit = shadowed["root"]["arguments"][1].clone();
    unit["id"] = json!("shadow_unit");
    shadowed["root"]["subcommands"][0]["arguments"] = json!([unit]);
    package.generate(
        &shadowed,
        &graph,
        &overlay,
        "invalid",
        Some("has incompatible argument"),
    )?;
    let mut unresolved = overlay.clone();
    unresolved["handlers"]["urn:field:read"] = json!(["device", "other"]);
    package.generate(
        &cmd,
        &graph,
        &unresolved,
        "invalid",
        Some("unresolved checked native command handler"),
    )?;
    let mut too_small = graph.clone();
    too_small["transport"]["maximum_frame_bytes"] = json!(1);
    super::project(
        cli,
        root,
        &too_small,
        "invalid",
        Some("constant request exceeds frame limit"),
    )?;
    println!(
        "field-meter IPC: literal and typed request frames, frame limits, argument checks, typed replies, and private endpoint discovery passed"
    );
    Ok(())
}
