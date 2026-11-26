//! Declaration-level, code-free Rust build inventory.
#[path = "../../verification/rust-recovery/shape_model.rs"]
mod shape_model;
mod structure;
pub mod tagged;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Profile {
    adapter: String,
    #[serde(default)]
    adapter_inputs: Vec<String>,
    source_revision: Option<String>,
    package: String,
    target_kind: TargetKind,
    target_name: String,
    target_triple: String,
    default_features: bool,
    #[serde(default)]
    features: Vec<String>,
    compiler_commit: String,
    cargo_lock_sha256: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TargetKind {
    Lib,
    Bin,
}

impl TargetKind {
    fn name(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Bin => "bin",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Pin<'a> {
    package: &'a str,
    source_revision: Option<&'a str>,
    target_kind: &'a str,
    target_name: &'a str,
    target_triple: &'a str,
    default_features: bool,
    features: &'a [String],
    compiler_commit: &'a str,
    cargo_lock_sha256: &'a str,
    manifest_sha256: String,
    package_manifest_sha256: String,
    adapter_sha256: String,
    adapter_inputs_sha256: BTreeMap<String, String>,
    cargo_config_sha256: BTreeMap<String, String>,
}

pub fn recover(manifest: &Path, profile_path: &Path, output: &Path) -> Result<()> {
    recover_profile(manifest, profile_path, output, false)
}

pub fn recover_shapes(manifest: &Path, profile_path: &Path, output: &Path) -> Result<()> {
    recover_profile(manifest, profile_path, output, true)
}

fn recover_profile(
    manifest: &Path,
    profile_path: &Path,
    output: &Path,
    data_shapes: bool,
) -> Result<()> {
    let manifest = manifest.canonicalize().map_err(|err| err.to_string())?;
    if manifest.file_name().and_then(|name| name.to_str()) != Some("Cargo.toml") {
        return Err("expected a Cargo.toml manifest".into());
    }
    let mut profile: Profile = toml::from_str(
        &fs::read_to_string(profile_path).map_err(|err| format!("recovery profile: {err}"))?,
    )
    .map_err(|err| format!("recovery profile: {err}"))?;
    profile.features.sort_unstable();
    if ![
        &profile.package,
        &profile.target_name,
        &profile.target_triple,
    ]
    .into_iter()
    .all(|value| cargo_name(value))
        || !profile.features.iter().all(|value| cargo_name(value))
        || profile.features.windows(2).any(|pair| pair[0] == pair[1])
        || !hex(&profile.compiler_commit, 40)
        || !hex(&profile.cargo_lock_sha256, 64)
        || profile
            .source_revision
            .as_deref()
            .is_some_and(|id| !hex(id, 40))
    {
        return Err("invalid recovery profile identifier, feature or digest".into());
    }
    let metadata = run(Command::new("cargo")
        .current_dir(manifest.parent().expect("manifest has a directory"))
        .arg("metadata")
        .args([
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&manifest))?;
    #[derive(Deserialize)]
    struct Workspace {
        workspace_root: std::path::PathBuf,
        packages: Vec<Package>,
    }
    #[derive(Deserialize)]
    struct Package {
        name: String,
        manifest_path: std::path::PathBuf,
    }
    let workspace: Workspace =
        serde_json::from_slice(&metadata).map_err(|err| format!("Cargo metadata: {err}"))?;
    let package = workspace
        .packages
        .iter()
        .find(|package| package.name == profile.package)
        .ok_or_else(|| format!("package {} is outside the workspace", profile.package))?;
    if let Some(revision) = &profile.source_revision {
        let git = |args: &[&str]| {
            run(Command::new("git")
                .current_dir(&workspace.workspace_root)
                .args(args))
        };
        if String::from_utf8_lossy(&git(&["rev-parse", "HEAD"])?).trim() != revision
            || !git(&["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty()
        {
            return Err("Rust source revision differs from the clean profile pin".into());
        }
    }
    let lock = workspace.workspace_root.join("Cargo.lock");
    if sha256(&fs::read(&lock).map_err(|err| format!("Cargo.lock: {err}"))?)
        != profile.cargo_lock_sha256
    {
        return Err("recovery profile Cargo.lock digest mismatch".into());
    }
    let adapter = profile_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(&profile.adapter)
        .canonicalize()
        .map_err(|err| format!("compiler adapter: {err}"))?;
    let adapter_directory = adapter.parent().expect("adapter has a directory");
    let mut adapter_inputs_sha256 = BTreeMap::new();
    for input in &profile.adapter_inputs {
        let path = crate::contracts::contained(adapter_directory, input)?;
        let digest =
            sha256(&fs::read(path).map_err(|err| format!("adapter input {input}: {err}"))?);
        if adapter_inputs_sha256
            .insert(input.clone(), digest)
            .is_some()
        {
            return Err("duplicate recovery adapter input".into());
        }
    }
    let mut cargo_config_sha256 = BTreeMap::new();
    record_cargo_configs(
        &mut cargo_config_sha256,
        "project",
        manifest.parent().expect("manifest directory"),
    )?;
    record_cargo_configs(
        &mut cargo_config_sha256,
        "driver",
        &env::current_dir().map_err(|err| err.to_string())?,
    )?;
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    if let Some(home) = cargo_home {
        record_config_dir(&mut cargo_config_sha256, "cargo-home", &home)?;
    }
    let pin = Pin {
        package: &profile.package,
        source_revision: profile.source_revision.as_deref(),
        target_kind: profile.target_kind.name(),
        target_name: &profile.target_name,
        target_triple: &profile.target_triple,
        default_features: profile.default_features,
        features: &profile.features,
        compiler_commit: &profile.compiler_commit,
        cargo_lock_sha256: &profile.cargo_lock_sha256,
        manifest_sha256: sha256(&fs::read(&manifest).map_err(|err| err.to_string())?),
        package_manifest_sha256: sha256(
            &fs::read(&package.manifest_path).map_err(|err| format!("package manifest: {err}"))?,
        ),
        adapter_inputs_sha256,
        cargo_config_sha256,
        adapter_sha256: sha256(&fs::read(&adapter).map_err(|err| err.to_string())?),
    };
    let features = if pin.default_features {
        "default"
    } else {
        "no-default"
    };
    let mut command = Command::new(adapter);
    command
        .arg(&manifest)
        .args([
            pin.package,
            pin.target_kind,
            pin.target_name,
            pin.target_triple,
            features,
        ])
        .args(pin.features);
    if data_shapes {
        command.env("ODD_RECOVERY_DATA_SHAPES", "1");
    } else {
        command.env_remove("ODD_RECOVERY_DATA_SHAPES");
    }
    let raw = run(&mut command)?;
    if data_shapes {
        return publish_shapes(output, &raw, pin);
    }
    let inventory = structure::Inventory::load(&raw)?;
    if inventory.crate_name != pin.target_name.replace('-', "_")
        || inventory.compiler.as_deref() != Some(pin.compiler_commit)
    {
        return Err("compiler adapter reported a different crate or compiler commit".into());
    }
    publish(output, &inventory, pin.package, Some(pin))
}

pub fn validate_inventory(path: &Path, output: &Path) -> Result<()> {
    validate_scoped_inventory(path, output, None)
}

pub fn validate_scoped_inventory(path: &Path, output: &Path, package: Option<&str>) -> Result<()> {
    let bytes = fs::read(path).map_err(|err| format!("candidate inventory: {err}"))?;
    let inventory = structure::Inventory::load(&bytes)?;
    let package = package.unwrap_or(&inventory.crate_name);
    if !cargo_name(package) {
        return Err("invalid inventory package name".into());
    }
    publish(output, &inventory, package, None)
}

fn record_cargo_configs(
    pins: &mut BTreeMap<String, String>,
    role: &str,
    directory: &Path,
) -> Result<()> {
    for (depth, ancestor) in directory.ancestors().enumerate() {
        record_config_dir(pins, &format!("{role}-{depth}"), &ancestor.join(".cargo"))?;
    }
    Ok(())
}

fn record_config_dir(
    pins: &mut BTreeMap<String, String>,
    role: &str,
    directory: &Path,
) -> Result<()> {
    for name in ["config", "config.toml"] {
        let path = directory.join(name);
        match fs::read(&path) {
            Ok(bytes) => {
                pins.insert(format!("{role}/{name}"), sha256(&bytes));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => {
                return Err(format!(
                    "read Cargo configuration {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn cargo_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}

fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .output()
        .map_err(|err| format!("{command:?}: {err}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "{command:?} failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn publish(
    output: &Path,
    inventory: &structure::Inventory,
    package: &str,
    pin: Option<Pin<'_>>,
) -> Result<()> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Lock<'a> {
        vocabulary: &'static str,
        pin: Option<Pin<'a>>,
        structure_sha256: String,
        declarations: usize,
        edges: usize,
        gaps: usize,
        qualification: &'static str,
    }
    let bytes = inventory.graph(package);
    let mut lock = serde_json::to_vec_pretty(&Lock {
        vocabulary: "oddlang-rust-structure-v1",
        pin,
        structure_sha256: sha256(&bytes),
        declarations: inventory.declarations.len(),
        edges: inventory.edges.len(),
        gaps: inventory.gaps.len(),
        qualification: "candidate",
    })
    .map_err(|err| err.to_string())?;
    lock.push(b'\n');
    publish_pair(output, "structure.ttl", bytes, "structure.lock", lock)
}

fn publish_pair(
    output: &Path,
    data_name: &str,
    bytes: Vec<u8>,
    lock_name: &str,
    lock: Vec<u8>,
) -> Result<()> {
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage =
        tempfile::tempdir_in(parent).map_err(|err| format!("stage recovery output: {err}"))?;
    fs::write(stage.path().join(data_name), bytes).map_err(|err| err.to_string())?;
    fs::write(stage.path().join(lock_name), lock).map_err(|err| err.to_string())?;
    fs::rename(stage.path(), output).map_err(|err| format!("publish recovery output: {err}"))
}

// Typed deserialization has already checked the model. In the older grammar
// the array discriminator was absent at every nesting level.
fn contains_array_shape(node: &serde_json::Value) -> bool {
    match node {
        serde_json::Value::Array(items) => items.iter().any(contains_array_shape),
        serde_json::Value::Object(fields) => {
            fields.get("kind").and_then(|kind| kind.as_str()) == Some("array")
                || fields.values().any(contains_array_shape)
        }
        _ => false,
    }
}

fn publish_shapes(output: &Path, raw: &[u8], pin: Pin<'_>) -> Result<()> {
    use shape_model::{Observation, PREVIOUS_VOCABULARY, VOCABULARY};

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Lock<'a> {
        vocabulary: &'a str,
        pin: Pin<'a>,
        shapes_sha256: String,
        qualification: &'static str,
    }

    let observation: Observation =
        serde_json::from_slice(raw).map_err(|err| format!("shape observation: {err}"))?;
    if observation.crate_name != pin.target_name.replace('-', "_")
        || observation.compiler != pin.compiler_commit
    {
        return Err("compiler adapter reported a different crate or compiler commit".into());
    }
    if !matches!(
        observation.graph.vocabulary.as_str(),
        VOCABULARY | PREVIOUS_VOCABULARY
    ) {
        return Err("compiler adapter reported an unsupported data shape graph".into());
    }
    let mut bytes = serde_json::to_vec_pretty(&observation.graph).map_err(|err| err.to_string())?;
    if observation.graph.vocabulary == PREVIOUS_VOCABULARY {
        let tree = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
        if contains_array_shape(&tree) {
            return Err("array shape requires the v2 data shape vocabulary".into());
        }
    }
    bytes.push(b'\n');
    let mut lock = serde_json::to_vec_pretty(&Lock {
        vocabulary: &observation.graph.vocabulary,
        pin,
        shapes_sha256: sha256(&bytes),
        qualification: "candidate",
    })
    .map_err(|err| err.to_string())?;
    lock.push(b'\n');
    publish_pair(output, "shapes.json", bytes, "shapes.lock", lock)
}
