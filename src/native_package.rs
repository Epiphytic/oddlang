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
pub(super) fn digest(bytes: &[u8]) -> String {
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
    #[serde(default)]
    binaries: Vec<Binary>,
    components: Vec<Component>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    name: String,
    entry: Vec<String>,
}

fn entrypoints(
    src: &Path,
    package: &str,
    binaries: &[Binary],
    files: &mut BTreeMap<String, String>,
) -> Result<()> {
    if binaries.is_empty() {
        return Ok(());
    }
    let library = package.replace('-', "_");
    if !ident(&library) {
        return Err("invalid native library import for executable".into());
    }
    io(fs::create_dir(src.join("bin")))?;
    for binary in binaries {
        if !ident(&binary.name.replace('-', "_"))
            || binary.entry.is_empty()
            || !binary.entry.iter().all(|s| ident(s))
        {
            return Err("invalid binary target or entry binding".into());
        }
        let entry = binary.entry.join("::");
        let source = format!(
            "fn main() -> ::std::process::ExitCode {{\n    let run: fn() -> ::std::process::ExitCode = ::{library}::{entry};\n    run()\n}}\n"
        );
        let rel = format!("bin/{}.rs", binary.name);
        if files
            .insert(format!("src/{rel}"), digest(source.as_bytes()))
            .is_some()
        {
            return Err(format!("duplicate binary target {}", binary.name));
        }
        io(fs::write(src.join(rel), source))?;
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    version: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(rename = "defaultFeatures")]
    default_features: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pin {
    pub(super) path: String,
    sha256: String,
}

impl Pin {
    pub(super) fn read(&self, base: &Path) -> Result<Vec<u8>> {
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
    IntegrityBlock,
    SharedDeclarations,
    SequenceJournal,
    CliSurface,
    LocalCommandClient,
    WireSchema,
    WireProtocol,
    BinaryRecord,
    FieldRelation,
    LocalStatusOwner,
    ForegroundProfileOwner,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Component {
    kind: Kind,
    module: String,
    graph: Pin,
    overlay: Pin,
    wire_shape: Option<WireShape>,
    reexports: Vec<String>,
    #[serde(default)]
    visibility: Visibility,
    root_reexports: Option<Vec<String>>,
    #[serde(default)]
    crate_reexports: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireShape {
    graph: Pin,
    overlay: Pin,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    #[default]
    Private,
    Public,
}

struct Group {
    directory: String,
    declarations: String,
    visibility: Visibility,
    exports: BTreeSet<String>,
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
    if component.wire_shape.is_some() && !matches!(component.kind, Kind::WireProtocol) {
        return Err("wireShape is only valid for wire-protocol components".into());
    }
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
        Kind::IntegrityBlock => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::integrity_block::build(&path, &overlay, &output)?;
            "codec.rs"
        }
        Kind::FieldRelation => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::field_relation::build(&path, &overlay, &output)?;
            "relations.rs"
        }
        Kind::BinaryRecord => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::binary_record::build(&path, &overlay, &output)?;
            "record.rs"
        }
        Kind::WireSchema => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::wire_schema::build(&path, &overlay, &output)?;
            "wire.rs"
        }
        Kind::WireProtocol => {
            let shape = component
                .wire_shape
                .as_ref()
                .ok_or("wire-protocol component requires wireShape")?;
            let path = input.join("graph.json");
            let shape_graph = input.join("shape.json");
            let shape_overlay = input.join("shape-rust.json");
            io(fs::write(&path, graph))?;
            io(fs::write(&shape_graph, shape.graph.read(base)?))?;
            io(fs::write(&shape_overlay, shape.overlay.read(base)?))?;
            crate::wire_schema::protocol::build(
                &shape_graph,
                &shape_overlay,
                &path,
                &overlay,
                &output,
            )?;
            "wire.rs"
        }
        Kind::SharedDeclarations => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::shared_declarations::build(&path, &overlay, &output)?;
            "shared.rs"
        }
        Kind::EndpointSet => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            let pin = crate::endpoint_set::digest(&path)?;
            let binding = crate::endpoint_set::overlay_digest(&overlay)?;
            crate::endpoint_set::build(&path, &pin, Some((&overlay, &binding)), &output)?;
            "endpoint_set.rs"
        }
        Kind::ForegroundProfileOwner => {
            crate::foreground_profile_owner::build_bytes(
                &graph,
                &io(fs::read(&overlay))?,
                &output,
            )?;
            "foreground.rs"
        }
        Kind::LocalStatusOwner => {
            crate::local_status_owner::build_bytes(&graph, &io(fs::read(&overlay))?, &output)?;
            "owner.rs"
        }
        Kind::LocalCommandClient => {
            crate::local_command_client::build_bytes(&graph, &io(fs::read(&overlay))?, &output)?;
            "client.rs"
        }
        Kind::CliSurface => {
            crate::command_surface::build_package_component(
                &graph,
                &io(fs::read(&overlay))?,
                &output,
            )?;
            "cli.rs"
        }
        Kind::SequenceJournal => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            let graph_pin = crate::sequence_journal::digest(&path)?;
            let binding_pin = crate::sequence_journal::overlay_digest(&overlay)?;
            crate::sequence_journal::build(&path, &graph_pin, &overlay, &binding_pin, &output)?;
            "sequence_journal.rs"
        }
    };
    let bytes = io(fs::read(output.join(source)))?;
    let lock = serde_json::from_slice(&io(fs::read(output.join("projection.lock")))?)
        .map_err(|e| format!("projection lock: {e}"))?;
    let parsed = syn::parse_file(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut public = BTreeSet::new();
    let mut crate_visible = BTreeSet::new();
    for item in &parsed.items {
        let (vis, id) = match item {
            syn::Item::Struct(i) => (&i.vis, &i.ident),
            syn::Item::Enum(i) => (&i.vis, &i.ident),
            syn::Item::Const(i) => (&i.vis, &i.ident),
            syn::Item::Type(i) => (&i.vis, &i.ident),
            syn::Item::Mod(i) => (&i.vis, &i.ident),
            syn::Item::Fn(i) => (&i.vis, &i.sig.ident),
            _ => continue,
        };
        match vis {
            syn::Visibility::Public(_) => {
                public.insert(id.to_string());
                crate_visible.insert(id.to_string());
            }
            syn::Visibility::Restricted(v) if v.path.is_ident("crate") => {
                crate_visible.insert(id.to_string());
            }
            _ => {}
        }
    }
    for (names, available, scope) in [
        (&component.reexports, &public, "public"),
        (&component.crate_reexports, &crate_visible, "crate-visible"),
    ] {
        if let Some(missing) = names.iter().find(|name| !available.contains(*name)) {
            return Err(format!("unknown {scope} projected type or item: {missing}"));
        }
    }
    Ok((bytes, lock))
}

fn export_paths(component: &Component, entry: &str) -> Vec<Vec<String>> {
    let entry = entry.to_owned();
    let public = component.reexports.contains(&entry);
    let mut paths = Vec::new();
    if public || component.crate_reexports.contains(&entry) {
        paths.push(vec![component.module.clone(), entry.clone()]);
    }
    if public
        && component
            .root_reexports
            .as_ref()
            .is_none_or(|roots| roots.contains(&entry))
    {
        paths.push(vec![entry]);
    }
    paths
}

fn validate_command_callbacks(components: &[Component], base: &Path) -> Result<()> {
    let mut callbacks = Vec::new();
    let status_owners = components
        .iter()
        .filter(|component| matches!(component.kind, Kind::LocalStatusOwner))
        .flat_map(|component| export_paths(component, "BoundOwner"))
        .collect::<BTreeSet<_>>();
    for component in components {
        let callback = match component.kind {
            Kind::LocalCommandClient => crate::local_command_client::callback(
                &component.graph.read(base)?,
                &component.overlay.read(base)?,
            )?,
            Kind::ForegroundProfileOwner => crate::foreground_profile_owner::callback(
                &component.graph.read(base)?,
                &component.overlay.read(base)?,
                &status_owners,
            )?,
            _ => continue,
        };
        callbacks.push((component, callback));
    }
    let mut exports = BTreeMap::new();
    for (component, callback) in &callbacks {
        for path in export_paths(component, &callback.entry) {
            exports.insert(path, callback);
        }
    }
    for component in components
        .iter()
        .filter(|component| matches!(component.kind, Kind::CliSurface))
    {
        crate::command_surface::validate_callbacks(
            &component.graph.read(base)?,
            &component.overlay.read(base)?,
            &exports,
        )?;
    }
    Ok(())
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
        let mut cargo_dependency = serde_json::json!({
            "version": format!("={}", dependency.version), "features": dependency.features
        });
        if let Some(enabled) = dependency.default_features {
            cargo_dependency["default-features"] = enabled.into();
        }
        dependencies.insert(name, cargo_dependency);
    }
    if manifest
        .components
        .iter()
        .any(|component| matches!(component.kind, Kind::CliSurface))
        && manifest
            .dependencies
            .get("clap")
            .is_none_or(|dependency| dependency.version != "4.6.7")
    {
        return Err("cli-surface requires the pinned clap 4.6.7 provider".into());
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
    build_pinned(&raw, base, output, true)
}
pub(super) fn build_pinned(raw: &[u8], base: &Path, output: &Path, standalone: bool) -> Result<()> {
    let manifest: Manifest =
        serde_json::from_slice(raw).map_err(|e| format!("package manifest: {e}"))?;
    validate_command_callbacks(&manifest.components, base)?;
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
    let mut groups = BTreeMap::<String, Group>::new();
    let mut exports: BTreeSet<_> = manifest
        .components
        .iter()
        .map(|c| c.module.clone())
        .collect();
    let mut generated_files = BTreeMap::new();
    let mut records = Vec::new();
    let mut root_exports = String::new();
    for (index, component) in manifest.components.iter().enumerate() {
        if matches!(component.kind, Kind::FieldRelation)
            && (!component.reexports.is_empty()
                || !component.crate_reexports.is_empty()
                || component
                    .root_reexports
                    .as_ref()
                    .is_some_and(|names| !names.is_empty()))
        {
            return Err("field relation component cannot reexport items".into());
        }

        if !ident(&component.module)
            || (component.reexports.is_empty()
                && component.crate_reexports.is_empty()
                && !matches!(component.kind, Kind::FieldRelation))
        {
            return Err("invalid module or empty reexports".into());
        }
        let group = groups
            .entry(component.module.clone())
            .or_insert_with(|| Group {
                directory: format!("group_{index}"),
                declarations: String::new(),
                visibility: component.visibility,
                exports: BTreeSet::new(),
            });
        if group.visibility != component.visibility {
            return Err(format!(
                "inconsistent group visibility: {}",
                component.module
            ));
        }
        let root_names = component
            .root_reexports
            .as_ref()
            .unwrap_or(&component.reexports);
        for name in component.reexports.iter().chain(&component.crate_reexports) {
            if !ident(name) || !group.exports.insert(name.clone()) {
                return Err(format!("invalid or conflicting reexport: {name}"));
            }
        }
        for name in root_names {
            if !component.reexports.contains(name) || !exports.insert(name.clone()) {
                return Err(format!("invalid or conflicting reexport: {name}"));
            }
        }
        let leaf = format!("__projection_{index}");
        let stage = work.path().join(&leaf);
        io(fs::create_dir(&stage))?;
        let (source, projection) = project(component, base, &stage)?;
        let directory = &group.directory;
        let module = src.join(directory);
        io(fs::create_dir_all(&module))?;
        io(fs::write(module.join(format!("{leaf}.rs")), &source))?;
        generated_files.insert(format!("src/{directory}/{leaf}.rs"), digest(&source));
        let names = component.reexports.join(", ");
        writeln!(group.declarations, "mod {leaf};").unwrap();
        if !names.is_empty() {
            writeln!(group.declarations, "pub use {leaf}::{{{names}}};").unwrap();
        }
        if !component.crate_reexports.is_empty() {
            writeln!(
                group.declarations,
                "pub(crate) use {leaf}::{{{}}};",
                component.crate_reexports.join(", ")
            )
            .unwrap();
        }
        if !root_names.is_empty() {
            writeln!(
                root_exports,
                "pub use {}::{{{}}};",
                component.module,
                root_names.join(", ")
            )
            .unwrap();
        }
        let mut record = serde_json::json!({"kind":component.kind,"module":component.module,"reexports":component.reexports,
            "graphSha256":component.graph.sha256,"overlaySha256":component.overlay.sha256,
            "sourceSha256":digest(&source),"projection":projection});
        if let Some(shape) = &component.wire_shape {
            record["wireShape"] = serde_json::json!({
                "graphSha256": shape.graph.sha256,
                "overlaySha256": shape.overlay.sha256,
            });
        }
        records.push(record);
    }
    let mut lib = String::new();
    for (
        group,
        Group {
            directory,
            declarations,
            visibility,
            ..
        },
    ) in groups
    {
        let prefix = if visibility == Visibility::Public {
            "pub "
        } else {
            ""
        };
        writeln!(
            lib,
            "#[path = \"{directory}/mod.rs\"]\n{prefix}mod {group};"
        )
        .unwrap();
        generated_files.insert(
            format!("src/{directory}/mod.rs"),
            digest(declarations.as_bytes()),
        );
        io(fs::write(src.join(directory).join("mod.rs"), declarations))?;
    }
    lib.push_str(&root_exports);
    generated_files.insert("src/lib.rs".into(), digest(lib.as_bytes()));
    io(fs::write(src.join("lib.rs"), lib))?;
    entrypoints(
        &src,
        &manifest.name,
        &manifest.binaries,
        &mut generated_files,
    )?;
    if standalone && !manifest.binaries.is_empty() {
        let checked = io(Command::new("cargo")
            .args(["check", "--bins", "--locked", "--offline", "--target-dir"])
            .arg(work.path().join("bin-check"))
            .current_dir(&package)
            .output())?;
        if !checked.status.success() {
            return Err(format!(
                "invalid binary entry binding: {}",
                String::from_utf8_lossy(&checked.stderr)
            ));
        }
    }
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
        "cargoLockSha256":digest(&lock),"cargoManifestSha256":digest(cargo.as_bytes()),"generatedFiles":generated_files,"components":records});
    io(fs::write(
        package.join("package.lock"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    ))?;
    publish(&package, output)
}
pub(crate) fn publish(package: &Path, output: &Path) -> Result<()> {
    // A competing creator can insert even an empty directory after the initial check.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, package, CWD, output, RenameFlags::NOREPLACE)
            .map_err(|error| format!("publish fresh package: {error}"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        io(fs::rename(package, output))
    }
}

#[cfg(test)]
mod binary_tests {
    use super::*;

    #[test]
    fn native_entrypoint_passes_process_exit_and_output() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='sensor-engine'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
        )
        .unwrap();
        fs::write(src.join("lib.rs"),"pub mod cli { pub fn status()->std::process::ExitCode { println!(\"ready\"); std::process::ExitCode::from(7) } }").unwrap();
        let mut files = BTreeMap::new();
        entrypoints(
            &src,
            "sensor-engine",
            &[Binary {
                name: "meter-check".into(),
                entry: vec!["cli".into(), "status".into()],
            }],
            &mut files,
        )
        .unwrap();
        assert_eq!(
            files["src/bin/meter-check.rs"],
            digest(&fs::read(src.join("bin/meter-check.rs")).unwrap())
        );
        let run = Command::new("cargo")
            .args(["run", "--offline", "--quiet", "--bin", "meter-check"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert_eq!(
            run.status.code(),
            Some(7),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"ready\n");
    }
}
