//! Exercise two independently named projections, package publication and a downstream caller.
mod native_artifacts;
use provider_validation::{Result, cli, command, pin, read, write};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
fn racing_publisher(cli: &Path, root: &Path, spec: &Path, manifest: &Value) -> Result {
    use std::os::unix::fs::symlink;
    write(spec, manifest)?;
    let paths = env::var_os("PATH").ok_or("missing PATH")?;
    let cargo = env::split_paths(&paths)
        .map(|p| p.join("cargo"))
        .find(|p| p.is_file())
        .ok_or("missing Cargo executable")?;
    let tools = root.join("tools");
    fs::create_dir(&tools)?;
    symlink(env::current_exe()?, tools.join("cargo"))?;
    let competing = root.join("competing");
    let mut run = Command::new(cli);
    run.env(
        "PATH",
        env::join_paths(std::iter::once(tools).chain(env::split_paths(&paths)))?,
    )
    .env("ODD_PACKAGE_TEST_COMPETING", &competing)
    .env("ODD_PACKAGE_TEST_CARGO", cargo);
    command(
        run.args(["build", "native-package", "rust"])
            .arg(spec)
            .arg(&competing),
        root,
        &[],
        Some("publish fresh package"),
    )?;
    assert!(competing.is_dir() && fs::read_dir(competing)?.next().is_none());
    Ok(())
}
fn main() -> Result {
    #[cfg(unix)]
    if let Some(dir) = env::var_os("ODD_PACKAGE_TEST_COMPETING")
        && env::args_os().next().is_some_and(|arg| {
            Path::new(&arg)
                .file_name()
                .is_some_and(|name| name == "cargo")
        })
    {
        use std::os::unix::process::CommandExt;
        // Cargo validation runs after the initial destination check and before the exclusive rename.
        fs::create_dir(dir)?;
        let error =
            Command::new(env::var_os("ODD_PACKAGE_TEST_CARGO").ok_or("missing real Cargo")?)
                .env_remove("ODD_PACKAGE_TEST_COMPETING")
                .env_remove("ODD_PACKAGE_TEST_CARGO")
                .args(env::args_os().skip(1))
                .exec();
        return Err(error.into());
    }
    let cli = cli()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/json-envelope");
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let inputs = root.join("inputs");
    fs::create_dir(&inputs)?;
    fs::copy(fixture.join("Cargo.lock"), inputs.join("Cargo.lock"))?;
    fs::copy(fixture.join("graph/domain.ttl"), inputs.join("domain.ttl"))?;
    let binding = read(&fixture.join("binding.json"))?;
    let cargo: Value = toml::from_str(&fs::read_to_string(fixture.join("Cargo.toml"))?)?;
    let mut dependencies = json!({});
    for (name, value) in cargo["dependencies"]
        .as_object()
        .ok_or("missing dependencies")?
    {
        let mut dep = if value.is_string() {
            json!({"version":value})
        } else {
            value.clone()
        };
        let version = dep["version"]
            .as_str()
            .ok_or("missing version pin")?
            .trim_start_matches('=')
            .to_owned();
        dep["version"] = json!(version);
        dependencies[name] = dep;
    }
    let mut components = Vec::new();
    for names in [
        ["Thumbnail", "Bounds", "ReadError"],
        ["Sticker", "StickerSize", "StickerError"],
    ] {
        let mut overlay = binding.clone();
        for (k, v) in ["codecType", "limitsType", "errorType"]
            .into_iter()
            .zip(names)
        {
            overlay[k] = json!(v);
        }
        let name = format!("{}.json", names[0]);
        write(&inputs.join(&name), &overlay)?;
        components.push(json!({"kind":"json-envelope","module":"lib","graph":pin(&inputs,"domain.ttl")?,"overlay":pin(&inputs,&name)?,"reexports":names}));
    }
    let manifest = json!({"format":"oddlang-native-package-v1","name":cargo["package"]["name"],"version":cargo["package"]["version"],
        "edition":"2024","dependencies":dependencies,"cargoLock":pin(&inputs,"Cargo.lock")?,"components":components});
    let spec = inputs.join("manifest.json");
    let build = |value: &Value, name: &str, error: Option<&str>| -> Result<PathBuf> {
        write(&spec, value)?;
        let dest = root.join(name);
        command(
            Command::new(&cli)
                .args(["build", "native-package", "rust"])
                .arg(&spec)
                .arg(&dest),
            root,
            &[],
            error,
        )?;
        if error.is_some() && name != "first" {
            assert!(!dest.exists());
        }
        Ok(dest)
    };
    let first = build(&manifest, "first", None)?;
    let repeat = build(&manifest, "second", None)?;
    let tree = native_artifacts::inventory(&first)?;
    assert_eq!(tree, native_artifacts::inventory(&repeat)?);
    let generated: serde_json::Map<_, _> = tree
        .into_iter()
        .filter(|(p, _)| p.starts_with("src/") && p.ends_with(".rs"))
        .map(|(k, v)| (k, json!(format!("{:x}", Sha256::digest(v)))))
        .collect();
    assert_eq!(
        read(&first.join("package.lock"))?["generatedFiles"],
        Value::Object(generated)
    );
    let mut suffix = BTreeSet::new();
    for entry in fs::read_dir(&inputs)? {
        suffix.insert(
            entry?
                .path()
                .extension()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
    }
    assert_eq!(
        suffix,
        ["ttl", "json", "lock"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    build(&manifest, "first", Some("output already exists"))?;
    let mut changed = manifest.clone();
    changed["components"][1]["reexports"] = json!(["Thumbnail"]);
    build(&changed, "duplicate", Some("conflicting reexport"))?;
    changed["components"][1]["reexports"] = json!(["Typo"]);
    build(&changed, "typo", Some("unknown public projected type"))?;
    let mut changed = manifest.clone();
    changed["cargoLock"]["sha256"] = json!("a".repeat(64));
    build(&changed, "stale", Some("digest mismatch"))?;
    fs::copy(inputs.join("domain.ttl"), root.join("outside.ttl"))?;
    {
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(root.join("outside.ttl"), inputs.join("alias.ttl"))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("outside.ttl"), inputs.join("alias.ttl"))?;
        let mut changed = manifest.clone();
        changed["components"][0]["graph"]["path"] = json!("alias.ttl");
        build(&changed, "escape", Some("outside package"))?;
    }
    let mut changed = manifest.clone();
    changed["version"] = json!("8.0.0");
    build(&changed, "stale-lock", Some("Cargo.lock validation"))?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    racing_publisher(&cli, root, &spec, &manifest)?;
    fs::create_dir(first.join("tests"))?;
    fs::write(
        first.join("tests/consumer.rs"),
        include_str!("../../fixtures/native-package/consumer.rs"),
    )?;
    command(
        &mut Command::new("cargo"),
        &first,
        &["test", "--locked", "--offline", "--quiet"],
        None,
    )?;
    println!(
        "Two projections in one private module, clean replay, negative controls and downstream Rust passed."
    );
    Ok(())
}
