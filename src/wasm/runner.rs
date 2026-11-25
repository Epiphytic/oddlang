//! Execute a pinned component through an independently built host. Only the
//! pinned corpus supplies expected outcomes. Reports contain observations.
use super::bundle::{self, Scenario, VerifiedBundle, digest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use wit_parser::{Resolve, WorldItem};

type Res<T> = Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn read(path: &Path) -> Res<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}
fn json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Res<T> {
    serde_json::from_slice(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}
fn write_json(path: &Path, value: &Value) -> Res<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut stage = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    serde_json::to_writer_pretty(&mut stage, value).map_err(err)?;
    stage.write_all(b"\n").map_err(err)?;
    stage.persist(path).map_err(err)?;
    Ok(())
}
fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Claim {
    participant: String,
    roles: Vec<String>,
    release_sha256: String,
    world: String,
    component_sha256: String,
    runner_sha256: String,
}
struct Release {
    bundle: VerifiedBundle,
    cases: Vec<Scenario>,
    wit: BTreeMap<String, Vec<u8>>,
}
impl Release {
    fn open(root: &Path, lock: &Path) -> Res<Self> {
        let (bundle, mut files) = bundle::checked(root, lock)?;
        let cases = serde_json::from_slice(&files.remove("cases.json").ok_or("missing cases")?)
            .map_err(err)?;
        let wit = files
            .into_iter()
            .filter_map(|(p, v)| p.strip_prefix("wit/").map(|s| (s.to_string(), v)))
            .collect();
        Ok(Self { bundle, cases, wit })
    }
    fn check(&self, claim: &Claim) -> Res<()> {
        if claim.participant.trim().is_empty()
            || !is_digest(&claim.component_sha256)
            || !is_digest(&claim.runner_sha256)
        {
            return Err("claim requires participant, component and runner digests".into());
        }
        if claim.release_sha256 != self.bundle.pin.manifest_sha256
            || claim.world != self.bundle.world.name()
        {
            return Err("claim release pin or world differs from verified release".into());
        }
        let available: BTreeSet<_> = self
            .cases
            .iter()
            .flat_map(|case| &case.covers)
            .map(|coverage| coverage.role.as_str())
            .collect();
        let roles: BTreeSet<_> = claim.roles.iter().map(String::as_str).collect();
        if roles.is_empty() || roles.len() != claim.roles.len() || !roles.is_subset(&available) {
            return Err("claim requires unique roles covered by the release".into());
        }
        Ok(())
    }
    fn selected<'a>(&'a self, claim: &Claim) -> Vec<&'a Scenario> {
        self.cases
            .iter()
            .filter(|case| {
                case.covers
                    .iter()
                    .any(|coverage| claim.roles.contains(&coverage.role))
            })
            .collect()
    }
}

fn compare(cases: &[&Scenario], observations: &Value) -> Res<()> {
    let observed = observations.as_array().ok_or("missing case observations")?;
    let mut by_id = BTreeMap::new();
    for case in observed {
        let id = case["caseId"].as_str().ok_or("missing observed caseId")?;
        if by_id.insert(id, case).is_some() {
            return Err(format!("duplicate observed case {id}"));
        }
    }
    for expected in cases {
        let id = &expected.case_id;
        let actual = by_id
            .remove(id.as_str())
            .ok_or_else(|| format!("missing observed case {id}"))?;
        if actual.as_object().is_none_or(|object| object.len() != 3) {
            return Err(format!("invalid observation fields in {id}"));
        }
        let calls: Vec<_> = expected
            .calls
            .iter()
            .map(|call| {
                json!({
                    "function": call.function,
                    "arguments": call.arguments,
                    "result": call.expected,
                })
            })
            .collect();
        for (field, entries) in [
            (
                "imports",
                serde_json::to_value(&expected.imports).map_err(err)?,
            ),
            ("calls", Value::Array(calls)),
        ] {
            let entries = entries.as_array().expect("serialized list");
            let observed = actual[field]
                .as_array()
                .ok_or_else(|| format!("missing {id}.{field}"))?;
            for (index, (expected, actual)) in entries.iter().zip(observed).enumerate() {
                if expected != actual {
                    return Err(format!(
                        "counterexample at {id}.{field}[{index}]: expected {expected}, observed {actual}"
                    ));
                }
            }
            if observed.len() != entries.len() {
                let index = entries.len().min(observed.len());
                return Err(format!(
                    "counterexample at {id}.{field}[{index}]: expected {:?}, observed {:?}",
                    entries.get(index),
                    observed.get(index)
                ));
            }
        }
    }
    if let Some(id) = by_id.keys().next() {
        return Err(format!("unexpected observed case {id}"));
    }
    Ok(())
}
fn evidence(claim: &Claim, observations: Value) -> Value {
    json!({"schemaVersion":1,"ruleSetVersion":1,"claim":claim,"cases":observations})
}
pub fn verify(bundle: &Path, lock: &Path, claim: &Path, report: &Path) -> Res<()> {
    let release = Release::open(bundle, lock)?;
    let claim: Claim = json_file(claim)?;
    release.check(&claim)?;
    let report: Value = json_file(report)?;
    if report != evidence(&claim, report["cases"].clone()) {
        return Err("evidence metadata differs from claim or rule set".into());
    }
    compare(&release.selected(&claim), &report["cases"])
}

#[derive(Serialize, PartialEq, Eq, Debug, Default)]
struct Surface {
    interfaces: BTreeSet<String>,
    imports: BTreeSet<String>,
    exports: BTreeSet<String>,
}
fn surface(path: &Path, world: Option<&str>) -> Res<Surface> {
    let mut resolve = Resolve::default();
    let (pkg, _) = resolve
        .push_path(path)
        .map_err(|e| format!("invalid WIT: {e:#}"))?;
    let world = resolve.select_world(&[pkg], world).map_err(err)?;
    let world = &resolve.worlds[world];
    let mut result = Surface::default();
    for (k, v) in &world.imports {
        match v {
            WorldItem::Interface { .. } => {
                result.interfaces.insert(resolve.name_world_key(k));
            }
            WorldItem::Function(_) => {
                result.imports.insert(resolve.name_world_key(k));
            }
            WorldItem::Type { .. } => {}
        }
    }
    result
        .exports
        .extend(world.exports.keys().map(|k| resolve.name_world_key(k)));
    Ok(result)
}

fn stage_wit(root: &Path, release: &Release) -> Res<Surface> {
    for (name, contents) in &release.wit {
        let path = root.join(name);
        fs::create_dir_all(path.parent().ok_or("invalid WIT path")?).map_err(err)?;
        fs::write(path, contents).map_err(err)?;
    }
    surface(root, Some(release.bundle.world.name()))
}
fn tool(command: &mut Command) -> Res<Vec<u8>> {
    let output = command
        .output()
        .map_err(|e| format!("cannot start {:?}: {e}", command.get_program()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(output.stdout)
}
struct Execution {
    temp: tempfile::TempDir,
    component: PathBuf,
    executable: PathBuf,
}
impl Execution {
    fn prepare(release: &Release, claim: &Claim, source: &Path) -> Res<Self> {
        let temp = tempfile::tempdir().map_err(err)?;
        let component = temp.path().join("component.wasm");
        let bytes = read(source)?;
        if digest(&bytes) != claim.component_sha256 {
            return Err("component digest differs from claim".into());
        }
        fs::write(&component, bytes).map_err(err)?;
        let executable = temp.path().join("runtime-host");
        let host = env::var_os("ODDLANG_WASM_HOST")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("examples/wasm/target/release/runtime-host"));
        fs::copy(&host, &executable).map_err(|e| format!("{}: {e}", host.display()))?;
        if digest(&read(&executable)?) != claim.runner_sha256 {
            return Err("runner digest differs from claim".into());
        }

        let tools = env::var_os("WASM_TOOLS").unwrap_or_else(|| "wasm-tools".into());
        tool(Command::new(&tools).arg("validate").arg(&component))?;
        let extracted = temp.path().join("extracted.wit");
        fs::write(
            &extracted,
            tool(
                Command::new(&tools)
                    .args(["component", "wit"])
                    .arg(&component),
            )?,
        )
        .map_err(err)?;
        let wit = temp.path().join("guest-wit");
        let declared = stage_wit(&wit, release)?;
        let actual = surface(&extracted, None)?;
        if actual != declared {
            return Err(format!(
                "component world differs from release: expected {declared:?}, observed {actual:?}"
            ));
        }
        tool(
            Command::new(&tools)
                .args([
                    "component",
                    "targets",
                    "--world",
                    release.bundle.world.name(),
                ])
                .arg(wit)
                .arg(&component),
        )
        .map_err(|e| format!("component ABI differs from release: {e}"))?;
        Ok(Self {
            temp,
            component,
            executable,
        })
    }
    fn observe(&self, release: &Release, cases: &[&Scenario]) -> Res<Value> {
        let mut request =
            serde_json::to_value(stage_wit(&self.temp.path().join("host-wit"), release)?)
                .map_err(err)?;
        // This matrix deliberately changes the tag at this stable declaration.
        request["numberFirst"] = json!(
            release
                .bundle
                .world
                .nodes
                .get("urn:oddlang:echo:number")
                .and_then(|node| node.props.get("ordinal"))
                .is_some_and(|ordinal| ordinal == "0")
        );
        let inputs = cases
            .iter()
            .map(|case| {
                let calls: Vec<_> = case
                    .calls
                    .iter()
                    .map(|call| {
                        json!({
                            "function": call.function, "arguments": call.arguments,
                        })
                    })
                    .collect();
                json!({"caseId": case.case_id, "imports": case.imports, "calls": calls})
            })
            .collect();
        request["cases"] = Value::Array(inputs);
        let file = self.temp.path().join("request.json");
        write_json(&file, &request)?;
        let bytes = tool(
            Command::new(&self.executable)
                .arg(&self.component)
                .arg(file),
        )?;
        serde_json::from_slice(&bytes).map_err(err)
    }
}
pub fn run(bundle: &Path, lock: &Path, claim: &Path, component: &Path, out: &Path) -> Res<()> {
    let release = Release::open(bundle, lock)?;
    let claim: Claim = json_file(claim)?;
    release.check(&claim)?;
    let execution = Execution::prepare(&release, &claim, component)?;
    let observed = execution.observe(&release, &release.selected(&claim))?;
    write_json(out, &evidence(&claim, observed))
}

pub fn matrix(
    host_paths: (&Path, &Path),
    guest_paths: (&Path, &Path),
    claim: &Path,
    component: &Path,
    direction: &str,
    out: &Path,
) -> Res<()> {
    if !matches!(direction, "old-host-new-guest" | "new-host-old-guest") {
        return Err("unknown matrix direction".into());
    }
    let host = Release::open(host_paths.0, host_paths.1)?;
    let guest = Release::open(guest_paths.0, guest_paths.1)?;
    let claim: Claim = json_file(claim)?;
    guest.check(&claim)?;
    let execution = Execution::prepare(&guest, &claim, component)?;
    let cases = host.selected(&claim);
    if cases.is_empty() {
        return Err("no host cases for selected participant roles".into());
    }
    let mut report = json!({"direction":direction,"hostReleaseSha256":host.bundle.pin.manifest_sha256,
        "guestReleaseSha256":guest.bundle.pin.manifest_sha256,"claim":claim,"ruleSetVersion":1,
        "caseIds": cases.iter().map(|case|&case.case_id).collect::<Vec<_>>()});
    match execution.observe(&host, &cases) {
        Ok(observed) => {
            match compare(&cases, &observed) {
                Ok(()) => report["verdict"] = json!("observed-compatible"),
                Err(reason) => {
                    report["verdict"] = json!("behavior-failure");
                    report["diagnostic"] = json!(reason);
                }
            }
            report["cases"] = observed;
        }
        Err(reason) => {
            let stage = if reason.starts_with("Error: load-failure") {
                "load-failure"
            } else if reason.starts_with("Error: link-failure") {
                "link-failure"
            } else if reason.starts_with("Error: type-failure") {
                "type-failure"
            } else {
                return Err(reason);
            };
            report["verdict"] = json!(stage);
            report["diagnostic"] = json!(reason);
        }
    }
    write_json(out, &report)
}
