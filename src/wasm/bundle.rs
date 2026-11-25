//! Deterministic, offline WIT releases and their declaration witnesses.
use super::model::World;
use crate::contracts::{Pin, contained, verified};
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

pub struct VerifiedBundle {
    pub pin: Pin,
    pub world: World,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Coverage {
    declaration: String,
    role: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Call {
    function: String,
    arguments: Vec<Value>,
    expected: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HostCall {
    function: String,
    arguments: Vec<Value>,
    returns: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Scenario {
    case_id: String,
    covers: Vec<Coverage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    constraints: Vec<String>,
    imports: Vec<HostCall>,
    calls: Vec<Call>,
}

fn digest(bytes: &[u8]) -> String {
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
    super::wit::import_dir(dir.path(), world.name(), &identities, None)?;
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
    Ok((manifest, files))
}

pub fn verify_bundle(root: &Path, lock_file: &Path) -> Result<VerifiedBundle, String> {
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
    Ok(VerifiedBundle {
        pin: pin.clone(),
        world,
    })
}

pub fn publish(
    source: &Path,
    dependency_lock: &Path,
    cache: &Path,
    out: &Path,
) -> Result<(), String> {
    let workspace = lock(dependency_lock)?;
    let model = read(&contained(source, "model.ttl")?)?;
    let world = World::from_turtle(&model)?;
    let mut cases: Vec<Scenario> =
        decode(&read(&contained(source, "corpus.json")?)?, "corpus.json")?;
    cases.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    for case in &mut cases {
        case.covers.sort();
        case.constraints.sort();
    }
    let direct: BTreeSet<String> = decode(
        &read(&contained(source, "dependencies.json")?)?,
        "dependencies.json",
    )?;
    let mut pending = direct;
    let mut closure = Pins::new();
    let mut files = Files::new();
    while let Some(id) = pending.pop_first() {
        if closure.contains_key(&id) {
            continue;
        }
        if id == world.identity() {
            return Err(format!("dependency cycle at {id}"));
        }
        let pin = workspace
            .get(&id)
            .ok_or_else(|| format!("missing dependency pin {id}"))?;
        let root = contained(
            cache,
            &format!("wasm/{}/{}", digest(id.as_bytes()), pin.version),
        )?;
        let (manifest, payloads) = verify_release(&root, pin, &workspace)?;
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
