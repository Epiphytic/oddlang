//! Deterministic, offline WIT releases and their declaration witnesses.
use super::model::World;
use crate::contracts::{Pin, contained, verified};
use oxigraph::{
    io::{RdfFormat, RdfParser},
    model::NamedOrBlankNode,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

type Files = BTreeMap<String, Vec<u8>>;
type Pins = BTreeMap<String, Pin>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    pins: Vec<Pin>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    rule_set_version: u32,
    identity: String,
    version: String,
    world: String,
    dependencies: Vec<Pin>,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
enum Dependency {
    Package(String),
    Include {
        identity: String,
        #[serde(rename = "includeWorld")]
        world: String,
    },
}
impl Dependency {
    fn identity(&self) -> &str {
        match self {
            Self::Package(id) | Self::Include { identity: id, .. } => id,
        }
    }
}

// Strip only the dependency's package header and entry-world marker. The
// extension can reference inherited IRIs but cannot change their definitions.
fn compose(source: &[u8], direct: &[Dependency], files: &Files) -> Result<Vec<u8>, String> {
    let mut model = source.to_vec();
    let mut owned = BTreeSet::new();
    let mut inherited = BTreeSet::new();
    let mut expected = BTreeSet::new();
    for dep in direct {
        let Dependency::Include { identity, world } = dep else {
            continue;
        };
        let bytes = required(
            files,
            &format!("dependencies/{}.ttl", digest(identity.as_bytes())),
        )?;
        let parent = World::from_turtle(bytes)?;
        if parent.identity() != identity || parent.selected != *world {
            return Err(format!(
                "included world {world} disagrees with pinned {identity}"
            ));
        }
        expected.insert(world);
        let (package, version) = parent
            .package_name()
            .rsplit_once('@')
            .ok_or("unversioned included WIT package")?;
        for (id, node) in &parent.nodes {
            if node.kind == "Interface" && !node.props.contains_key("from") {
                let from = oxigraph::model::Literal::new_simple_literal(format!(
                    "{package}/{}@{version}",
                    node.name
                ));
                inherited.insert(format!("<{id}> <urn:oddlang:wasm:from> {from} .\n"));
            }
        }
        for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(bytes) {
            let q = quad.map_err(|e| e.to_string())?;
            let NamedOrBlankNode::NamedNode(id) = &q.subject else {
                return Err("expected named inherited declaration".into());
            };
            if id.as_str() == parent.identity() || q.predicate.as_str() == "urn:oddlang:wasm:entry"
            {
                continue;
            }
            owned.insert(id.as_str().to_owned());
            inherited.insert(format!("{} {} {} .\n", q.subject, q.predicate, q.object));
        }
    }
    if expected.is_empty() {
        return Ok(model);
    }
    for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(source) {
        let q = quad.map_err(|e| e.to_string())?;
        if let NamedOrBlankNode::NamedNode(id) = q.subject
            && owned.contains(id.as_str())
        {
            return Err(format!("extension redefines pinned declaration {id}"));
        }
    }
    model.push(b'\n');
    for triple in inherited {
        model.extend(triple.as_bytes())
    }
    let merged = World::from_turtle(&model)?;
    for world in expected {
        if !merged.nodes[&merged.selected]
            .targets("include")
            .any(|target| target == world)
        {
            return Err(format!("missing include edge to pinned world {world}"));
        }
    }
    Ok(model)
}

pub struct VerifiedBundle {
    pub pin: Pin,
    pub world: World,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(super) struct Coverage {
    pub(super) declaration: String,
    pub(super) role: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Call {
    pub(super) function: String,
    pub(super) arguments: Vec<Value>,
    pub(super) expected: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HostCall {
    pub(super) function: String,
    pub(super) arguments: Vec<Value>,
    pub(super) returns: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Scenario {
    pub(super) case_id: String,
    pub(super) covers: Vec<Coverage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) constraints: Vec<String>,
    pub(super) imports: Vec<HostCall>,
    pub(super) calls: Vec<Call>,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn decode<T: DeserializeOwned>(bytes: &[u8], name: &str) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("{name}: {e}"))
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn insert_pin(pins: &mut Pins, pin: Pin) -> Result<(), String> {
    oxigraph::model::NamedNode::new(&pin.identity).map_err(|e| e.to_string())?;
    if pin.manifest_sha256.len() != 64
        || !pin
            .manifest_sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(format!("invalid manifest digest for {}", pin.identity));
    }
    if pin.version.is_empty()
        || !pin
            .version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+'))
    {
        return Err(format!("invalid version for {}", pin.identity));
    }
    if let Some(previous) = pins.get(&pin.identity)
        && previous != &pin
    {
        return Err(format!("conflicting pins for {}", pin.identity));
    }
    pins.insert(pin.identity.clone(), pin);
    Ok(())
}

fn lock(path: &Path) -> Result<Pins, String> {
    let lock: Lock = decode(&read(path)?, "lock")?;
    let mut pins = Pins::new();
    for pin in lock.pins {
        insert_pin(&mut pins, pin)?;
    }
    Ok(pins)
}

fn obligations(world: &World, cases: &[Scenario], wit: &[u8]) -> Result<Value, String> {
    let mut rows = Vec::new();
    let wit_hash = digest(wit);
    let identities = world.identities()?;
    for (path, iri) in &identities {
        rows.push(json!({"declaration":iri,"kind":"structure","witness":{"surfacePath":path,"witSha256":wit_hash}}));
    }
    let required = world.behavior_roles()?;
    let mut functions = BTreeMap::new();
    for (path, iri) in &identities {
        let parts: Vec<_> = path.split('/').collect();
        let (dir, name) = match parts.as_slice() {
            ["world", dir, name] => (*dir, name.to_string()),
            ["world", dir, iface, "function", name] => (*dir, format!("{iface}/{name}")),
            _ => continue,
        };
        let node = &world.nodes[iri];
        if node.kind == "Function" && matches!(dir, "import" | "export") {
            functions.insert((dir.to_owned(), name), (iri, node.targets("param").count()));
        }
    }
    let bounds: BTreeMap<_, _> = world
        .nodes
        .iter()
        .filter_map(|(iri, node)| {
            let min = node.props.get("min");
            let max = node.props.get("max");
            (min.is_some() || max.is_some()).then_some((iri, (min, max)))
        })
        .collect();
    let mut bound_cases = BTreeMap::<_, BTreeSet<_>>::new();
    let mut ids = BTreeSet::new();
    let mut associations = BTreeMap::<_, BTreeSet<_>>::new();
    for case in cases {
        if case.case_id.is_empty()
            || !ids.insert(&case.case_id)
            || case.covers.is_empty()
            || case.calls.is_empty()
        {
            return Err(format!(
                "duplicate, empty or unassociated case {}",
                case.case_id
            ));
        }
        for constraint in &case.constraints {
            if !bounds.contains_key(constraint)
                || !bound_cases
                    .entry(constraint)
                    .or_default()
                    .insert(&case.case_id)
            {
                return Err(format!(
                    "unknown or duplicate constraint {constraint} in {}",
                    case.case_id
                ));
            }
        }
        let mut exercised = BTreeSet::new();
        for (dir, function, arity) in case
            .calls
            .iter()
            .map(|c| ("export", &c.function, c.arguments.len()))
            .chain(
                case.imports
                    .iter()
                    .map(|c| ("import", &c.function, c.arguments.len())),
            )
        {
            let (id, expected_arity) = functions
                .get(&(dir.to_owned(), function.clone()))
                .ok_or_else(|| format!("unknown {dir} {function} in {}", case.case_id))?;
            if arity != *expected_arity {
                return Err(format!(
                    "wrong argument count for {function} in {}",
                    case.case_id
                ));
            }
            exercised.insert(((*id).clone(), format!("wasm.{dir}er")));
        }
        for cover in &case.covers {
            if !exercised.contains(&(cover.declaration.clone(), cover.role.clone())) {
                return Err(format!("unknown declaration or role in {}", case.case_id));
            }
            let entry = associations
                .entry((cover.declaration.clone(), cover.role.clone()))
                .or_default();
            if !entry.insert(&case.case_id) {
                return Err(format!("duplicate case association {}", case.case_id));
            }
        }
    }
    for pair in required {
        let cases = associations
            .get(&pair)
            .ok_or_else(|| format!("uncovered {} {}", pair.0, pair.1))?;
        rows.push(json!({"declaration":pair.0,"role":pair.1,"kind":"behavior","caseIds":cases}));
    }
    for (iri, (min, max)) in bounds {
        let cases = bound_cases
            .get(iri)
            .ok_or_else(|| format!("uncovered bounds {iri}"))?;
        rows.push(json!({"declaration":iri,"kind":"bounds","min":min,"max":max,"caseIds":cases}));
    }
    Ok(Value::Array(rows))
}

fn write_files(root: &Path, files: &Files) -> Result<(), String> {
    for (name, bytes) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap())
            .and_then(|_| fs::write(path, bytes))
            .map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

fn required<'a>(files: &'a Files, name: &str) -> Result<&'a [u8], String> {
    files
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("unlisted {name}"))
}

fn validate_world(files: &Files, world: &World) -> Result<(), String> {
    let wit = required(files, "wit/world.wit")?;
    let identities = world.identities()?;
    if wit != world.render()?.as_bytes()
        || required(files, "identities.json")? != encode(&identities)?
    {
        return Err("WIT or stable identities disagree with ontology".into());
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let wit_files = files
        .iter()
        .filter_map(|(name, bytes)| {
            name.strip_prefix("wit/")
                .map(|path| (path.to_owned(), bytes.clone()))
        })
        .collect();
    write_files(dir.path(), &wit_files)?;
    // Resolve only verified bytes. Unlisted files beside the bundle cannot satisfy imports.
    let recovered = super::wit::import_dir(dir.path(), world.name(), &identities, None)
        .map_err(|message| message.replace(dir.path().to_string_lossy().as_ref(), "wit"))?;
    let report = super::diff::compare(world, &recovered, super::diff::Direction::OldHostNewGuest)?;
    if report.verdict != "surface-equal" {
        return Err(format!(
            "resolved WIT disagrees with declared surface: {}",
            report
        ));
    }
    Ok(())
}

fn verify_release(root: &Path, pin: &Pin, workspace: &Pins) -> Result<(Manifest, Files), String> {
    let bytes = verified(root, "manifest.json", &pin.manifest_sha256)?;
    let manifest: Manifest = decode(&bytes, "manifest.json")?;
    if manifest.schema_version != 1
        || manifest.rule_set_version != 1
        || manifest.identity != pin.identity
        || manifest.version != pin.version
    {
        return Err("manifest disagrees with pin or unsupported rule version".into());
    }
    let mut closure = workspace.clone();
    insert_pin(&mut closure, pin.clone())?;
    for dep in &manifest.dependencies {
        if dep.identity == pin.identity {
            return Err(format!("dependency cycle at {}", pin.identity));
        }
        insert_pin(&mut closure, dep.clone())?;
        if workspace.get(&dep.identity) != Some(dep) {
            return Err(format!("missing dependency pin {}", dep.identity));
        }
    }
    let mut files = Files::new();
    for (name, hash) in &manifest.files {
        files.insert(name.clone(), verified(root, name, hash)?);
    }
    let mut expected_wit = BTreeSet::from(["wit/world.wit".to_owned()]);
    let pins: BTreeMap<_, _> = manifest
        .dependencies
        .iter()
        .map(|p| (&p.identity, p))
        .collect();
    for dep in &manifest.dependencies {
        let key = digest(dep.identity.as_bytes());
        let bytes = required(&files, &format!("dependencies/{key}.json"))?;
        let original: Manifest = decode(bytes, "dependency manifest")?;
        if digest(bytes) != dep.manifest_sha256
            || original.identity != dep.identity
            || original.version != dep.version
        {
            return Err(format!(
                "embedded manifest disagrees with pin {}",
                dep.identity
            ));
        }
        for nested in original.dependencies {
            if pins.get(&nested.identity) != Some(&&nested) {
                return Err(format!(
                    "missing or conflicting transitive pin {}",
                    nested.identity
                ));
            }
        }
        let name = format!("wit/deps/{key}.wit");
        if original.files.get("wit/world.wit") != Some(&digest(required(&files, &name)?)) {
            return Err(format!("embedded WIT disagrees with pin {}", dep.identity));
        }
        expected_wit.insert(name);
    }
    if files
        .keys()
        .filter(|k| k.starts_with("wit/"))
        .cloned()
        .collect::<BTreeSet<_>>()
        != expected_wit
    {
        return Err("unexpected WIT file outside dependency closure".into());
    }
    let direct: Vec<Dependency> =
        decode(required(&files, "dependencies.json")?, "dependencies.json")?;
    for dep in &direct {
        let pin = pins
            .get(&dep.identity().to_owned())
            .ok_or_else(|| format!("missing dependency pin {}", dep.identity()))?;
        if let Dependency::Include { .. } = dep {
            let key = digest(pin.identity.as_bytes());
            let original: Manifest = decode(
                required(&files, &format!("dependencies/{key}.json"))?,
                "dependency manifest",
            )?;
            if original.files.get("model.ttl")
                != Some(&digest(required(
                    &files,
                    &format!("dependencies/{key}.ttl"),
                )?))
            {
                return Err(format!(
                    "included ontology disagrees with pin {}",
                    pin.identity
                ));
            }
        }
    }
    if direct
        .iter()
        .any(|dep| matches!(dep, Dependency::Include { .. }))
        && compose(required(&files, "source.ttl")?, &direct, &files)?
            != required(&files, "model.ttl")?
    {
        return Err("included ontology disagrees with selected release".into());
    }
    Ok((manifest, files))
}

pub(super) fn checked(root: &Path, lock_file: &Path) -> Result<(VerifiedBundle, Files), String> {
    let workspace = lock(lock_file)?;
    let header: Manifest = decode(&read(&contained(root, "manifest.json")?)?, "manifest.json")?;
    let pin = workspace
        .get(&header.identity)
        .ok_or_else(|| format!("missing release pin {}", header.identity))?;
    let (manifest, files) = verify_release(root, pin, &workspace)?;
    let world = World::from_turtle(required(&files, "model.ttl")?)?;
    if manifest.identity != world.identity()
        || manifest.version != world.version()
        || manifest.world != world.name()
    {
        return Err("release metadata disagrees with ontology".into());
    }
    validate_world(&files, &world)?;
    let cases: Vec<Scenario> = decode(required(&files, "cases.json")?, "cases.json")?;
    if required(&files, "obligations.json")?
        != encode(&obligations(
            &world,
            &cases,
            required(&files, "wit/world.wit")?,
        )?)?
    {
        return Err("invalid obligation inventory".into());
    }
    Ok((
        VerifiedBundle {
            pin: pin.clone(),
            world,
        },
        files,
    ))
}

pub fn verify_bundle(root: &Path, lock: &Path) -> Result<VerifiedBundle, String> {
    Ok(checked(root, lock)?.0)
}
pub(crate) fn verified_wit(root: &Path, lock: &Path) -> Result<(World, Files), String> {
    let (release, files) = checked(root, lock)?;
    let wit = files
        .into_iter()
        .filter_map(|(path, bytes)| {
            path.strip_prefix("wit/")
                .map(|name| (name.to_owned(), bytes))
        })
        .collect();
    Ok((release.world, wit))
}

pub fn publish(
    source: &Path,
    dependency_lock: &Path,
    cache: &Path,
    out: &Path,
) -> Result<(), String> {
    let workspace = lock(dependency_lock)?;
    let authored = read(&contained(source, "model.ttl")?)?;
    let mut cases: Vec<Scenario> =
        decode(&read(&contained(source, "corpus.json")?)?, "corpus.json")?;
    cases.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    for case in &mut cases {
        case.covers.sort();
        case.constraints.sort();
    }
    let mut direct: Vec<Dependency> = decode(
        &read(&contained(source, "dependencies.json")?)?,
        "dependencies.json",
    )?;
    direct.sort();
    let mut pending: BTreeSet<String> =
        direct.iter().map(|dep| dep.identity().to_owned()).collect();
    if pending.len() != direct.len() {
        return Err("duplicate direct package dependency".into());
    }
    let includes: BTreeSet<_> = direct
        .iter()
        .filter_map(|dep| match dep {
            Dependency::Include { identity, .. } => Some(identity.clone()),
            _ => None,
        })
        .collect();
    let mut closure = Pins::new();
    let mut files = Files::new();
    while let Some(id) = pending.pop_first() {
        if closure.contains_key(&id) {
            continue;
        }
        let pin = workspace
            .get(&id)
            .ok_or_else(|| format!("missing dependency pin {id}"))?;
        let root = contained(
            cache,
            &format!("wasm/{}/{}", digest(id.as_bytes()), pin.version),
        )?;
        let (manifest, payloads) = verify_release(&root, pin, &workspace)?;
        if includes.contains(&id) {
            files.insert(
                format!("dependencies/{}.ttl", digest(id.as_bytes())),
                required(&payloads, "model.ttl")?.to_vec(),
            );
        }
        for dep in manifest.dependencies {
            if workspace.get(&dep.identity) != Some(&dep) {
                return Err(format!(
                    "missing or conflicting transitive pin {}",
                    dep.identity
                ));
            }
            pending.insert(dep.identity);
        }
        files.insert(
            format!("dependencies/{}.json", digest(id.as_bytes())),
            verified(&root, "manifest.json", &pin.manifest_sha256)?,
        );
        files.insert(
            format!("wit/deps/{}.wit", digest(id.as_bytes())),
            required(&payloads, "wit/world.wit")?.to_vec(),
        );
        insert_pin(&mut closure, pin.clone())?;
    }
    let model = compose(&authored, &direct, &files)?;
    let world = World::from_turtle(&model)?;
    if closure.contains_key(world.identity()) {
        return Err(format!("dependency cycle at {}", world.identity()));
    }
    if !includes.is_empty() {
        files.insert("source.ttl".into(), authored);
    }
    files.insert("dependencies.json".into(), encode(&direct)?);
    let wit = world.render()?.into_bytes();
    files.insert(
        "obligations.json".into(),
        encode(&obligations(&world, &cases, &wit)?)?,
    );
    files.insert("wit/world.wit".into(), wit);
    files.insert("model.ttl".into(), model);
    files.insert("cases.json".into(), encode(&cases)?);
    files.insert("identities.json".into(), encode(&world.identities()?)?);
    validate_world(&files, &world)?;
    let manifest = encode(&Manifest {
        schema_version: 1,
        rule_set_version: 1,
        identity: world.identity().into(),
        version: world.version().into(),
        world: world.name().into(),
        dependencies: closure.values().cloned().collect(),
        files: files
            .iter()
            .map(|(name, bytes)| (name.clone(), digest(bytes)))
            .collect(),
    })?;
    insert_pin(
        &mut closure,
        Pin {
            identity: world.identity().into(),
            version: world.version().into(),
            manifest_sha256: digest(&manifest),
        },
    )?;
    files.insert("manifest.json".into(), manifest);
    files.insert(
        "workspace.lock.json".into(),
        encode(&Lock {
            pins: closure.into_values().collect(),
        })?,
    );
    if out.exists()
        && fs::read_dir(out)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("publication needs an empty output directory".into());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix(".wasm-release-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    write_files(stage.path(), &files)?;
    fs::rename(stage.path(), out).map_err(|e| format!("publish {}: {e}", out.display()))
}
