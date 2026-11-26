//! Assemble a native library from pinned semantic components and Rust overlays.
mod recovered;
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
struct Manifest<C = Component> {
    format: String,
    name: String,
    version: String,
    edition: String,
    #[serde(default)]
    dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    registry_patches: BTreeMap<String, GitSource>,
    cargo_lock: Pin,
    #[serde(default)]
    binaries: Vec<Binary>,
    components: Vec<C>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    name: String,
    entry: Vec<String>,
    startup: Option<Startup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Startup {
    dependency: String,
    entry: Vec<String>,
    exit: u8,
    stderr: String,
}

impl Startup {
    fn render(&self, siblings: Option<&BTreeSet<String>>) -> Result<String> {
        let name = self.dependency.replace('-', "_");
        if !ident(&name) || !siblings.is_some_and(|edges| edges.contains(&self.dependency)) {
            return Err("binary startup dependency must be a declared workspace sibling".into());
        }
        if self.entry.is_empty()
            || !self.entry.iter().all(|s| ident(s))
            || self.exit == 0
            || self.stderr.is_empty()
            || self.stderr.chars().any(char::is_control)
        {
            return Err("invalid binary startup binding or rejection".into());
        }
        let entry = self.entry.join("::");
        Ok(format!(
            "    let start: fn() -> ::std::result::Result<(), _> = ::{name}::{entry};\n    if start().is_err() {{\n        eprintln!(\"{{}}\", {:?});\n        return ::std::process::ExitCode::from({});\n    }}\n",
            self.stderr, self.exit,
        ))
    }
}

fn entrypoints(
    src: &Path,
    package: &str,
    binaries: &[Binary],
    siblings: Option<&BTreeSet<String>>,
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
        let startup = binary
            .startup
            .as_ref()
            .map(|hook| hook.render(siblings))
            .transpose()?
            .unwrap_or_default();
        let source = format!(
            "fn main() -> ::std::process::ExitCode {{\n{startup}    let run: fn() -> ::std::process::ExitCode = ::{library}::{entry};\n    run()\n}}\n"
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
    #[serde(rename = "defaultFeatures", alias = "default_features")]
    default_features: Option<bool>,
    source: Option<GitSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GitSource {
    git: String,
    revision: String,
}

impl GitSource {
    fn cargo(&self) -> Result<serde_json::Value> {
        if !self.git.strip_prefix("https://").is_some_and(|url| {
            url.split_once('/')
                .is_some_and(|(host, path)| !host.is_empty() && !path.is_empty())
                && !url.chars().any(|ch| {
                    ch.is_control() || ch.is_whitespace() || matches!(ch, '@' | '?' | '#')
                })
        }) || self.revision.len() != 40
            || !self
                .revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(
                "git dependency requires an HTTPS repository and full lowercase revision".into(),
            );
        }
        Ok(serde_json::json!({"git":self.git,"rev":self.revision}))
    }
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
    BufferedInput,
    BuildReport,
    StructuredDiagnostics,
    ProcessLimit,
    PublicRecords,
    ValueDomain,
    JsonEnvelope,
    EndpointSet,
    IntegrityBlock,
    SharedDeclarations,
    LineGrammar,
    CheckedValues,
    FiniteErrorDomain,
    SequenceJournal,
    CliSurface,
    LocalCommandClient,
    WireSchema,
    WireProtocol,
    BinaryRecord,
    BinaryFragment,
    BinaryFrame,
    FieldRelation,
    LocalStatusOwner,
    ForegroundProfileOwner,
    FileProfile,
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
    #[serde(skip)]
    recovery: Option<recovered::Provenance>,
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

pub(crate) fn ident(name: &str) -> bool {
    name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name != "gen"
        && syn::parse_str::<syn::Ident>(name).is_ok()
}

fn project(
    component: &Component,
    base: &Path,
    stage: &Path,
    dependencies: &BTreeMap<String, Dependency>,
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
        Kind::BufferedInput => {
            crate::buffered_input::build_bytes(&graph, &io(fs::read(&overlay))?, &output)?;
            "buffer.rs"
        }
        Kind::StructuredDiagnostics => {
            if !dependencies.contains_key("serde_json") {
                return Err(
                    "structured-diagnostics requires a pinned serde_json dependency".into(),
                );
            }
            crate::structured_diagnostics::build_bytes(&graph, &io(fs::read(&overlay))?, &output)?;
            "diagnostics.rs"
        }
        Kind::BuildReport => {
            crate::build_report::build(&graph, &io(fs::read(&overlay))?, &output)?;
            "report.rs"
        }
        Kind::PublicRecords => {
            let serde_available = dependencies
                .get("serde")
                .is_some_and(|dep| dep.features.iter().any(|feature| feature == "derive"));
            let assertion = component
                .recovery
                .as_ref()
                .and_then(|recovery| recovery.source_api.as_ref())
                .map(|pin| pin.read(base))
                .transpose()?;
            crate::public_records::build_bytes(
                &graph,
                &io(fs::read(&overlay))?,
                assertion.as_deref(),
                &output,
                serde_available,
            )?;
            "records.rs"
        }
        Kind::ProcessLimit => {
            crate::process_limit::build(
                &graph,
                &io(fs::read(&overlay))?,
                &output,
                dependencies.contains_key("libc"),
            )?;
            "limits.rs"
        }
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
        Kind::BinaryFrame => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::binary_frame::build(&path, &overlay, &output)?;
            "frame.rs"
        }
        Kind::BinaryRecord | Kind::BinaryFragment => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            if matches!(component.kind, Kind::BinaryFragment) {
                crate::binary_record::build_bound_fragment(&path, &overlay, &output)?;
            } else {
                crate::binary_record::build(&path, &overlay, &output)?;
            }
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
        Kind::FiniteErrorDomain => {
            crate::finite_error_domain::build_bytes(&graph, &io(fs::read(&overlay))?, &output)?;
            "errors.rs"
        }
        Kind::CheckedValues => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::checked_values::build(&path, &overlay, &output)?;
            "values.rs"
        }
        Kind::LineGrammar => {
            let path = input.join("graph.json");
            io(fs::write(&path, graph))?;
            crate::line_grammar::build(&path, &overlay, &output)?;
            "lines.rs"
        }
        Kind::SharedDeclarations => {
            if component.recovery.is_some()
                && !dependencies
                    .get("serde")
                    .is_some_and(|dep| dep.features.iter().any(|feature| feature == "derive"))
            {
                return Err("recovered string choice requires native serde with derive".into());
            }
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
        Kind::FileProfile => {
            crate::file_profile::build_bytes(&graph, &io(fs::read(&overlay))?, &output)?;
            "profile.rs"
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

pub(crate) struct ErrorVariantExport {
    pub id: String,
    pub error_type: String,
    pub variant: String,
    pub unit: bool,
    pub static_text: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RegistryProvider {
    pub name: &'static str,
    version: &'static str,
    checksum: &'static str,
}

pub(super) const ZEROIZE: RegistryProvider = RegistryProvider {
    name: "zeroize",
    version: "1.9.1",
    checksum: "e13084392c5e4bc371903e2935a5eaeed24905a7511356b883835e18a78f6879",
};

impl RegistryProvider {
    pub fn verify_lock(&self, bytes: &[u8]) -> Result<()> {
        let lock = std::str::from_utf8(bytes)
            .map_err(|e| e.to_string())?
            .parse::<toml::Table>()
            .map_err(|e| e.to_string())?;
        let packages = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or("missing Cargo.lock package entries")?;
        let mut entries = packages.iter().filter(|entry| {
            entry.get("name").and_then(toml::Value::as_str) == Some(self.name)
                && entry.get("version").and_then(toml::Value::as_str) == Some(self.version)
        });
        if !entries.next().is_some_and(|entry| {
            entry.get("source").and_then(toml::Value::as_str)
                == Some("registry+https://github.com/rust-lang/crates.io-index")
                && entry.get("checksum").and_then(toml::Value::as_str) == Some(self.checksum)
        }) || entries.next().is_some()
        {
            return Err(format!(
                "expected locked registry source and checksum for {} {}",
                self.name, self.version
            ));
        }
        Ok(())
    }
}

fn require_erasing_storage(manifest: &Manifest, component: &str) -> Result<()> {
    if manifest.registry_patches.contains_key(ZEROIZE.name)
        || !manifest
            .dependencies
            .get(ZEROIZE.name)
            .is_some_and(|dep| dep.version == ZEROIZE.version && dep.source.is_none())
    {
        return Err(format!(
            "{component} requires the locked registry provider zeroize 1.9.1"
        ));
    }
    Ok(())
}

fn validate_line_grammar_errors(components: &[Component], base: &Path) -> Result<()> {
    for parser in components
        .iter()
        .filter(|component| matches!(component.kind, Kind::LineGrammar))
    {
        let Some(link) = crate::line_grammar::error_link(&parser.overlay.read(base)?)? else {
            continue;
        };
        let mut matches = 0;
        for component in components {
            let error_variants = match component.kind {
                Kind::SharedDeclarations => crate::shared_declarations::error_variants,
                Kind::FiniteErrorDomain => crate::finite_error_domain::error_variants,
                _ => continue,
            };
            for error in
                error_variants(&component.graph.read(base)?, &component.overlay.read(base)?)?
            {
                if error.id != link.id {
                    continue;
                }
                let paths = export_paths(component, &error.error_type);
                if !error.static_text
                    || error.variant != link.variant
                    || !paths.iter().any(|path| {
                        link.r#type.first().is_some_and(|root| root == "crate")
                            && link.r#type[1..] == path[..]
                    })
                {
                    return Err(format!(
                        "line grammar error binding does not match a native export: {}",
                        link.id
                    ));
                }
                matches += 1;
            }
        }
        if matches != 1 {
            return Err(format!(
                "line grammar requires exactly one bound native error export: {}",
                link.id
            ));
        }
    }
    Ok(())
}

fn validate_public_record_links(components: &[Component], base: &Path) -> Result<()> {
    for record in components
        .iter()
        .filter(|part| matches!(part.kind, Kind::PublicRecords))
    {
        let links = crate::public_records::external_choice_links(
            &record.graph.read(base)?,
            &record.overlay.read(base)?,
        )?;
        if links.is_empty() {
            continue;
        }
        let mut available = BTreeMap::new();
        for part in components
            .iter()
            .filter(|part| matches!(part.kind, Kind::SharedDeclarations))
        {
            for (id, name) in crate::shared_declarations::string_choice_names(
                &part.graph.read(base)?,
                &part.overlay.read(base)?,
            )? {
                if available
                    .insert(id.clone(), export_paths(part, &name))
                    .is_some()
                {
                    return Err(format!("ambiguous external public record choice: {id}"));
                }
            }
        }
        for (id, path) in links {
            if !available
                .get(&id)
                .is_some_and(|paths| paths.iter().any(|actual| actual[..] == path[1..]))
            {
                return Err(format!("unresolved external public record choice: {id}"));
            }
        }
    }
    Ok(())
}

fn validate_component_requirements(
    manifest: &Manifest,
    base: &Path,
) -> Result<BTreeSet<RegistryProvider>> {
    let mut required = BTreeSet::new();
    let components = &manifest.components;
    let checked: Vec<_> = components
        .iter()
        .filter(|component| matches!(component.kind, Kind::CheckedValues))
        .collect();
    if components
        .iter()
        .any(|c| matches!(c.kind, Kind::BufferedInput))
    {
        require_erasing_storage(manifest, "buffered-input")?;
        required.insert(ZEROIZE);
    }
    let mut limits = Vec::new();
    for component in components
        .iter()
        .filter(|component| matches!(component.kind, Kind::ProcessLimit))
    {
        let overlay = component.overlay.read(base)?;
        if let Some(error) = crate::process_limit::declared_error(&overlay)? {
            limits.push(error);
        }
    }
    if checked.is_empty() && limits.is_empty() {
        return Ok(required);
    }
    let mut providers = BTreeMap::new();
    for component in components {
        let error_variants = match component.kind {
            Kind::SharedDeclarations => crate::shared_declarations::error_variants,
            Kind::FiniteErrorDomain => crate::finite_error_domain::error_variants,
            _ => continue,
        };
        for error in error_variants(&component.graph.read(base)?, &component.overlay.read(base)?)? {
            let paths = export_paths(component, &error.error_type)
                .into_iter()
                .filter(|_| error.unit)
                .map(|path| format!("crate::{}::{}", path.join("::"), error.variant))
                .collect::<BTreeSet<_>>();
            if providers.insert(error.id.clone(), paths).is_some() {
                return Err(format!("ambiguous native error declaration: {}", error.id));
            }
        }
    }
    for component in checked {
        if crate::checked_values::needs_erasure_provider(&component.graph.read(base)?)? {
            require_erasing_storage(manifest, "fixed-bytes")?;
            required.insert(ZEROIZE);
        }
        crate::checked_values::validate_error_links(&component.overlay.read(base)?, &providers)?;
    }
    for error in limits {
        error.validate_link(&providers)?;
    }
    Ok(required)
}

fn validate_command_callbacks(components: &[Component], base: &Path) -> Result<()> {
    let mut callbacks = Vec::new();
    let status_owners = components
        .iter()
        .filter(|component| matches!(component.kind, Kind::LocalStatusOwner))
        .flat_map(|component| export_paths(component, "BoundOwner"))
        .collect::<BTreeSet<_>>();
    let mut retained_stores = BTreeMap::new();
    for component in components
        .iter()
        .filter(|component| matches!(component.kind, Kind::FileProfile))
    {
        if let Some(identity) = crate::file_profile::retained_stores(
            &component.graph.read(base)?,
            &component.overlay.read(base)?,
        )? {
            for path in export_paths(component, "open_retained_stores") {
                if retained_stores.insert(path, identity.clone()).is_some() {
                    return Err("duplicate retained store export".into());
                }
            }
        }
    }
    for component in components {
        let callback = match component.kind {
            Kind::BuildReport => crate::build_report::callback(
                &component.graph.read(base)?,
                &component.overlay.read(base)?,
            )?,
            Kind::LocalCommandClient => crate::local_command_client::callback(
                &component.graph.read(base)?,
                &component.overlay.read(base)?,
            )?,
            Kind::ForegroundProfileOwner => crate::foreground_profile_owner::callback(
                &component.graph.read(base)?,
                &component.overlay.read(base)?,
                &status_owners,
                &retained_stores,
            )?,
            Kind::FileProfile => crate::file_profile::callback(
                &component.graph.read(base)?,
                &component.overlay.read(base)?,
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
        let mut record = match &dependency.source {
            Some(source) => source.cargo()?,
            None => serde_json::json!({}),
        };
        record["version"] = format!("={}", dependency.version).into();
        record["features"] = serde_json::json!(dependency.features);
        if let Some(default_features) = dependency.default_features {
            record["default-features"] = default_features.into();
        }
        dependencies.insert(name, record);
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
    let mut cargo = serde_json::json!({"package":{"name":manifest.name,"version":manifest.version,
        "edition":manifest.edition,"publish":false},"dependencies":dependencies,"workspace":{}});
    if !manifest.registry_patches.is_empty() {
        let mut patches = BTreeMap::new();
        for (name, source) in &manifest.registry_patches {
            if !valid_name(name) {
                return Err(format!("invalid registry patch crate: {name}"));
            }
            patches.insert(name, source.cargo()?);
        }
        cargo["patch"] = serde_json::json!({"crates-io":patches});
    }
    toml::to_string_pretty(&cargo).map_err(|e| format!("Cargo manifest: {e}"))
}

pub fn build(path: &Path, output: &Path) -> Result<()> {
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let path = io(path.canonicalize())?;
    let base = path.parent().ok_or("missing manifest directory")?;
    let raw = io(fs::read(&path))?;
    build_pinned(&raw, base, output, None, DependencyCache::Offline).map(|_| ())
}
#[derive(Clone, Copy)]
pub(super) enum DependencyCache {
    Offline,
    FetchLocked,
}

impl DependencyCache {
    pub(super) fn resolve(self, directory: &Path, context: &str) -> Result<()> {
        let mut cargo = Command::new("cargo");
        cargo.args(["metadata", "--locked", "--format-version", "1"]);
        if matches!(self, Self::Offline) {
            cargo.arg("--offline");
        }
        let result = io(cargo.current_dir(directory).output())?;
        if !result.status.success() {
            return Err(format!(
                "{context}: {}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        Ok(())
    }
}

pub(super) fn build_pinned(
    raw: &[u8],
    base: &Path,
    output: &Path,
    siblings: Option<&BTreeSet<String>>,
    cache: DependencyCache,
) -> Result<BTreeSet<RegistryProvider>> {
    let input: Manifest<recovered::ComponentInput> =
        serde_json::from_slice(raw).map_err(|e| format!("package manifest: {e}"))?;
    let manifest = input.resolve(base)?;
    validate_command_callbacks(&manifest.components, base)?;
    let required = validate_component_requirements(&manifest, base)?;
    validate_line_grammar_errors(&manifest.components, base)?;
    validate_public_record_links(&manifest.components, base)?;
    let cargo = cargo_manifest(&manifest)?;
    let lock = manifest.cargo_lock.read(base)?;
    for provider in &required {
        provider.verify_lock(&lock)?;
    }
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
    if manifest
        .components
        .iter()
        .any(|c| matches!(c.kind, Kind::BuildReport))
    {
        for dependency in ["clap", "serde_json"] {
            if !manifest.dependencies.contains_key(dependency) {
                return Err(format!(
                    "build report requires native dependency {dependency}"
                ));
            }
        }
        let script = crate::build_report::TARGET_SCRIPT;
        io(fs::write(package.join("build.rs"), script))?;
        generated_files.insert("build.rs".into(), digest(script.as_bytes()));
    }
    let mut records = Vec::new();
    let mut root_exports = String::new();
    for (index, component) in manifest.components.iter().enumerate() {
        if matches!(component.kind, Kind::BufferedInput)
            && (component.visibility != Visibility::Private
                || !component.reexports.is_empty()
                || !component.crate_reexports.is_empty()
                || component
                    .root_reexports
                    .as_ref()
                    .is_some_and(|names| !names.is_empty()))
        {
            return Err("buffered-input must stay private".into());
        }
        if matches!(component.kind, Kind::FieldRelation | Kind::BinaryFragment)
            && (!component.reexports.is_empty()
                || !component.crate_reexports.is_empty()
                || component
                    .root_reexports
                    .as_ref()
                    .is_some_and(|names| !names.is_empty()))
        {
            return Err("field relation or binary fragment component cannot reexport items".into());
        }

        if !ident(&component.module)
            || (component.reexports.is_empty()
                && component.crate_reexports.is_empty()
                && !matches!(
                    component.kind,
                    Kind::FieldRelation | Kind::BinaryFragment | Kind::BufferedInput
                ))
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
        let (source, projection) = project(component, base, &stage, &manifest.dependencies)?;
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
        if let Some(provenance) = &component.recovery {
            record["recoverySha256"] = serde_json::json!(provenance.sha256);
            record["recoveredDeclaration"] = serde_json::json!(provenance.declaration);
            if let Some(assertion) = &provenance.source_api {
                record["sourceApiSha256"] = serde_json::json!(assertion.sha256);
            }
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
        siblings,
        &mut generated_files,
    )?;
    if siblings.is_none() && !manifest.binaries.is_empty() {
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
    cache.resolve(&package, "Cargo.lock validation")?;
    let receipt = serde_json::json!({"format":"oddlang-native-package-lock-v1","manifestSha256":digest(&raw),
        "cargoLockSha256":digest(&lock),"cargoManifestSha256":digest(cargo.as_bytes()),"generatedFiles":generated_files,"components":records});
    io(fs::write(
        package.join("package.lock"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    ))?;
    publish(&package, output)?;
    Ok(required)
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
                startup: None,
            }],
            None,
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

#[cfg(test)]
mod git_sources_tests {
    use super::GitSource;

    #[test]
    fn admits_revision_pinned_https_only() {
        let git = "https://example.invalid/team/library".to_owned();
        let revision = "a".repeat(40);
        let source = GitSource {
            git: git.clone(),
            revision: revision.clone(),
        };
        assert_eq!(source.cargo().unwrap()["rev"], revision);
        for source in [
            GitSource {
                git: git.clone(),
                revision: "main".into(),
            },
            GitSource {
                git: "file:///checkout".into(),
                revision: revision.clone(),
            },
            GitSource {
                git: "https://secret@example.invalid/lib".into(),
                revision,
            },
        ] {
            assert!(source.cargo().is_err());
        }
    }
}
