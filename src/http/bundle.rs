use super::*;
use crate::contracts::Pin;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub type CaseId = String;
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimPair {
    pub operation: String,
    pub role: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claim {
    pub schema_version: u32,
    pub claims: Vec<ClaimPair>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaseResult {
    pub case_id: String,
    pub outcome: String,
    pub observations: Vec<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub schema_version: u32,
    pub release: Pin,
    pub rule_set_version: u32,
    pub claims: Vec<ClaimPair>,
    pub results: Vec<CaseResult>,
}
#[derive(Clone, Debug)]
pub struct VerifiedBundle {
    pub pin: Pin,
    pub rule_set_version: u32,
    pub obligations: Vec<Obligation>,
    pub cases: Vec<Case>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn payload(v: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn associations(inventory: &[Obligation], cases: &[Case]) -> Result<Value, String> {
    if let Some(gap) = gaps(inventory, cases).first() {
        return Err(format!("uncovered witness {gap:?}"));
    }
    let mut index = BTreeMap::<Slot, Vec<&str>>::new();
    let allowed: BTreeSet<_> = inventory
        .iter()
        .flat_map(|o| o.slots.iter().map(|k| o.witness(k)))
        .collect();
    let mut ids = BTreeSet::new();
    for c in cases {
        if !super::wire::valid_case(c) {
            return Err(format!("invalid case exchanges or action {}", c.case_id));
        }
        if !ids.insert(&c.case_id) || c.slots.is_empty() {
            return Err(format!("duplicate or unassociated case {}", c.case_id));
        }
        for s in &c.slots {
            if s.operation != c.operation || s.role != c.role || !allowed.contains(s) {
                return Err(format!("invalid case association {}", c.case_id));
            }
            index.entry(s.clone()).or_default().push(&c.case_id);
        }
    }
    Ok(json!(
        inventory
            .iter()
            .map(|o| {
                let mut row = json!(o);
                row["slots"] = json!(
                    o.slots
                        .iter()
                        .map(|kind| json!({"kind":kind,"caseIds":index[&o.witness(kind)]}))
                        .collect::<Vec<_>>()
                );
                row
            })
            .collect::<Vec<_>>()
    ))
}
/// Run the same witness and association gate used by publication.
pub fn validate_coverage(inventory: &[Obligation], corpus: &[Case]) -> Result<(), String> {
    associations(inventory, corpus).map(|_| ())
}
pub fn publish(c: &HttpContract, out: &Path) -> Result<(), HttpError> {
    if let Some(name) = c.source_files.keys().find(|p| !normal(p)) {
        return Err(format!("invalid payload path {name}"));
    }
    let corpus = cases(c)?;
    let required = inventory(c);
    let mut files = c.source_files.clone();
    for (path, bytes) in [
        ("cases.json", payload(&corpus)?),
        (
            "obligations.json",
            payload(&associations(&required, &corpus)?)?,
        ),
        ("schema.json", payload(&schema::schema(c))?),
        ("openapi.json", payload(&schema::openapi(c))?),
    ] {
        if files.insert(path.into(), bytes).is_some() {
            return Err(format!("reserved payload path {path}"));
        }
    }
    if files
        .keys()
        .any(|p| p == "manifest.json" || p == "workspace.lock.json")
    {
        return Err("reserved manifest or lock path".into());
    }
    let hashes: BTreeMap<_, _> = files.iter().map(|(p, b)| (p, digest(b))).collect();
    let manifest = payload(
        &json!({"schemaVersion":1,"identity":c.identity.as_str(),"version":c.version,"entry":c.entry,"ruleSetVersion":c.rule_set_version,"dependencies":c.dependencies,"files":hashes}),
    )?;
    let pin = Pin {
        identity: c.identity.to_string(),
        version: c.version.clone(),
        manifest_sha256: digest(&manifest),
    };
    files.insert("manifest.json".into(), manifest);
    files.insert(
        "workspace.lock.json".into(),
        payload(&json!({"pins":[pin]}))?,
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
        .prefix(".oddlang-release-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    for (name, bytes) in files {
        let path = stage.path().join(name);
        fs::create_dir_all(path.parent().unwrap())
            .and_then(|()| fs::write(path, bytes))
            .map_err(|e| e.to_string())?;
    }
    fs::rename(stage.path(), out).map_err(|e| format!("publish {}: {e}", out.display()))
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
pub fn verify_bundle(root: &Path, pin_file: &Path) -> Result<VerifiedBundle, String> {
    let lock = lock(pin_file)?;
    let path = contained(root, "manifest.json")?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let pin = lock.get(&m.identity).ok_or("missing HTTP release pin")?;
    if pin.manifest_sha256 != digest(&bytes)
        || m.version != pin.version
        || m.schema_version != 1
        || m.rule_set_version != 1
    {
        return Err("manifest disagrees with pin or unsupported version".into());
    }
    for (identity, dependency) in pins(m.dependencies)? {
        if lock
            .get(&identity)
            .is_some_and(|actual| actual != &dependency)
        {
            return Err(format!("conflicting dependency pin for {identity}"));
        }
    }
    let mut files = BTreeMap::new();
    for (path, hash) in &m.files {
        files.insert(path, verified(root, path, hash)?);
    }
    let get = |name: &str| {
        files
            .get(&name.to_owned())
            .ok_or_else(|| format!("unlisted {name}"))
    };
    for name in [&m.entry, "schema.json", "openapi.json"] {
        get(name)?;
    }
    let cases =
        serde_json::from_slice::<Vec<Case>>(get("cases.json")?).map_err(|e| e.to_string())?;
    let rows: Vec<Value> =
        serde_json::from_slice(get("obligations.json")?).map_err(|e| e.to_string())?;
    let mut obligations = vec![];
    for mut row in rows.clone() {
        let slots = row["slots"]
            .as_array()
            .ok_or("invalid witness slots")?
            .iter()
            .map(|s| s["kind"].clone())
            .collect();
        row["slots"] = Value::Array(slots);
        obligations.push(serde_json::from_value(row).map_err(|e| e.to_string())?);
    }
    if associations(&obligations, &cases)? != json!(rows) {
        return Err("invalid obligation case associations".into());
    }
    Ok(VerifiedBundle {
        pin: pin.clone(),
        rule_set_version: m.rule_set_version,
        cases,
        obligations,
    })
}
pub fn select_cases(b: &VerifiedBundle, claim: &Claim) -> Result<Vec<CaseId>, String> {
    if claim.schema_version != 1 || claim.claims.is_empty() {
        return Err("invalid claim".into());
    }
    let mut claims = BTreeSet::new();
    for c in &claim.claims {
        if !claims.insert(c)
            || !b
                .obligations
                .iter()
                .any(|o| o.operation == c.operation && o.role == c.role)
        {
            return Err(format!("unknown or duplicate claim {c:?}"));
        }
    }
    Ok(b.cases
        .iter()
        .filter(|c| {
            claims.contains(&ClaimPair {
                operation: c.operation.clone(),
                role: c.role.clone(),
            })
        })
        .map(|c| c.case_id.clone())
        .collect())
}
pub fn verify_evidence(b: &VerifiedBundle, claim: &Claim, e: &Evidence) -> Result<(), String> {
    let required: BTreeSet<_> = select_cases(b, claim)?.into_iter().collect();
    let mut claims = e.claims.clone();
    claims.sort();
    let mut expected = claim.claims.clone();
    expected.sort();
    if e.schema_version != 1
        || e.release != b.pin
        || e.rule_set_version != b.rule_set_version
        || claims != expected
    {
        return Err("evidence release, rules or claims disagree".into());
    }
    let mut seen = BTreeSet::new();
    for r in &e.results {
        if !required.contains(&r.case_id) || !seen.insert(r.case_id.clone()) {
            return Err(format!("unexpected or duplicate case {}", r.case_id));
        }
        let case = b.cases.iter().find(|c| c.case_id == r.case_id).unwrap();
        if r.outcome != "pass" || !compare_observations(&case.expected, &r.observations) {
            return Err(format!("failed observations {}", r.case_id));
        }
    }
    if let Some(missing) = required.difference(&seen).next() {
        return Err(format!("unexecuted {missing}"));
    }
    Ok(())
}
