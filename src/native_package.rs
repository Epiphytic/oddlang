//! Assemble a native library from pinned semantic components and Rust overlays.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Component as PathComponent, Path},
    process::Command,
};

type Result<T> = std::result::Result<T, String>;
fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|error| error.to_string())
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    name: String,
    version: String,
    edition: String,
    #[serde(default)]
    dependencies: BTreeMap<String, Dependency>,
    cargo_lock: Pin,
    components: Vec<Component>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    version: String,
    #[serde(default)]
    features: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    path: String,
    sha256: String,
}

impl Pin {
    fn read(&self, base: &Path) -> Result<Vec<u8>> {
        let path = Path::new(&self.path);
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|v| !matches!(v, PathComponent::Normal(_)))
        {
            return Err(format!("expected relative package input: {}", self.path));
        }
        let full = io(base.join(path).canonicalize())?;
        if !full.starts_with(base) || !full.is_file() {
            return Err(format!(
                "input outside package or not a file: {}",
                self.path
            ));
        }
        let bytes = io(fs::read(full))?;
        if digest(&bytes) != self.sha256 {
            return Err(format!("input digest mismatch: {}", self.path));
        }
        Ok(bytes)
    }
}

#[derive(Deserialize, Serialize, Copy, Clone)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    ValueDomain,
    JsonEnvelope,
    EndpointSet,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    kind: Kind,
    module: String,
    graph: Pin,
    overlay: Pin,
    reexports: Vec<String>,
}

fn ident(name: &str) -> bool {
    name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name != "gen"
        && syn::parse_str::<syn::Ident>(name).is_ok()
}

fn project(
    component: &Component,
    base: &Path,
    stage: &Path,
) -> Result<(Vec<u8>, serde_json::Value)> {
    let graph = component.graph.read(base)?;
    let overlay = stage.join("rust.json");
    io(fs::write(&overlay, component.overlay.read(base)?))?;
    let input = stage.join("graph");
    io(fs::create_dir(&input))?;
    let output = stage.join("projection");
    let source = match component.kind {
        Kind::ValueDomain | Kind::JsonEnvelope => {
            io(fs::write(input.join("domain.ttl"), graph))?;
            match component.kind {
                Kind::ValueDomain => {
                    crate::value_domain::build(&input, &overlay, &output)?;
                    "value.rs"
                }
                _ => {
                    crate::json_envelope::build(&input, &overlay, &output)?;
                    "codec.rs"
                }
            }
        }
        Kind::EndpointSet => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            let pin = crate::endpoint_set::digest(&path)?;
            let binding = crate::endpoint_set::overlay_digest(&overlay)?;
            crate::endpoint_set::build(&path, &pin, Some((&overlay, &binding)), &output)?;
            "endpoint_set.rs"
        }
    };
    let bytes = io(fs::read(output.join(source)))?;
    let lock = serde_json::from_slice(&io(fs::read(output.join("projection.lock")))?)
        .map_err(|e| format!("projection lock: {e}"))?;
    let parsed = syn::parse_file(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let public: BTreeSet<_> = parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                Some(item.ident.to_string())
            }
            syn::Item::Enum(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                Some(item.ident.to_string())
            }
            _ => None,
        })
        .collect();
    if let Some(missing) = component
        .reexports
        .iter()
        .find(|name| !public.contains(*name))
    {
        return Err(format!("unknown public projected type: {missing}"));
    }
    Ok((bytes, lock))
}

fn cargo_manifest(manifest: &Manifest) -> Result<String> {
    let valid_name = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
    };
    if manifest.format != "oddlang-native-package-v1"
        || !valid_name(&manifest.name)
        || !matches!(manifest.edition.as_str(), "2021" | "2024")
        || manifest.components.is_empty()
    {
        return Err("invalid package format, name, edition or empty component list".into());
    }
    semver::Version::parse(&manifest.version).map_err(|e| format!("package version: {e}"))?;
    let mut dependencies = BTreeMap::new();
    for (name, dependency) in &manifest.dependencies {
        if !valid_name(name) {
            return Err(format!("invalid dependency: {name}"));
        }
        semver::Version::parse(&dependency.version).map_err(|e| format!("{name} version: {e}"))?;
        dependencies.insert(name, serde_json::json!({"version":format!("={}",dependency.version),"features":dependency.features}));
    }
    let cargo = serde_json::json!({"package":{"name":manifest.name,"version":manifest.version,
        "edition":manifest.edition,"publish":false},"dependencies":dependencies,"workspace":{}});
    toml::to_string_pretty(&cargo).map_err(|e| format!("Cargo manifest: {e}"))
}

pub fn build(path: &Path, output: &Path) -> Result<()> {
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let path = io(path.canonicalize())?;
    let base = path.parent().ok_or("missing manifest directory")?;
    let raw = io(fs::read(&path))?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|e| format!("package manifest: {e}"))?;
    let cargo = cargo_manifest(&manifest)?;
    let lock = manifest.cargo_lock.read(base)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let work = io(tempfile::tempdir_in(parent))?;
    let package = work.path().join("package");
    let src = package.join("src");
    io(fs::create_dir_all(&src))?;
    io(fs::write(package.join("Cargo.toml"), &cargo))?;
    io(fs::write(package.join("Cargo.lock"), &lock))?;
    let mut groups = BTreeMap::<String, String>::new();
    let mut exports: BTreeSet<_> = manifest
        .components
        .iter()
        .map(|c| c.module.clone())
        .collect();
    let mut records = Vec::new();
    let mut root_exports = String::new();
    for (index, component) in manifest.components.iter().enumerate() {
        if !ident(&component.module) || component.reexports.is_empty() {
            return Err("invalid module or empty reexports".into());
        }
        for name in &component.reexports {
            if !ident(name) || !exports.insert(name.clone()) {
                return Err(format!("invalid or conflicting reexport: {name}"));
            }
        }
        let leaf = format!("__projection_{index}");
        let stage = work.path().join(&leaf);
        io(fs::create_dir(&stage))?;
        let (source, projection) = project(component, base, &stage)?;
        let module = src.join(&component.module);
        io(fs::create_dir_all(&module))?;
        io(fs::write(module.join(format!("{leaf}.rs")), &source))?;
        let names = component.reexports.join(", ");
        writeln!(
            groups.entry(component.module.clone()).or_default(),
            "mod {leaf};\npub use {leaf}::{{{names}}};"
        )
        .unwrap();
        writeln!(root_exports, "pub use {}::{{{names}}};", component.module).unwrap();
        records.push(serde_json::json!({"kind":component.kind,"module":component.module,"reexports":component.reexports,
            "graphSha256":component.graph.sha256,"overlaySha256":component.overlay.sha256,
            "sourceSha256":digest(&source),"projection":projection}));
    }
    let mut lib = String::new();
    for (group, declarations) in groups {
        writeln!(lib, "mod {group};").unwrap();
        io(fs::write(src.join(&group).join("mod.rs"), declarations))?;
    }
    lib.push_str(&root_exports);
    io(fs::write(src.join("lib.rs"), lib))?;
    // Cargo validates the dependency closure and resolves the exact external crates without building.
    let result = io(Command::new("cargo")
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
        .current_dir(&package)
        .output())?;
    if !result.status.success() {
        return Err(format!(
            "Cargo.lock validation: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let receipt = serde_json::json!({"format":"oddlang-native-package-lock-v1","manifestSha256":digest(&raw),
        "cargoLockSha256":digest(&lock),"cargoManifestSha256":digest(cargo.as_bytes()),"components":records});
    io(fs::write(
        package.join("package.lock"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    ))?;
    io(fs::rename(package, output))
}
