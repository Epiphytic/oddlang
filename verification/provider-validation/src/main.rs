//! Independently compare all directories, file names, permission bits and bytes.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::tempdir;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const LOCK: &str = "2efe4cea5e65e5918024f7f79465c92297e47dc019a66399b4e5894eb6eb172c";
const INPUTS: &str = include_str!("../marmish-inputs.sha256");

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Eq, PartialEq)]
struct Node {
    mode: u32,
    digest: Option<String>,
}

fn visit(root: &Path, relative: &Path, tree: &mut BTreeMap<PathBuf, Node>) -> Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let kind = entry.file_type()?;
        let digest = if kind.is_dir() {
            visit(root, &path, tree)?;
            None
        } else if kind.is_file() {
            Some(sha(&fs::read(entry.path())?))
        } else {
            return Err(format!("unexpected file type: {}", path.display()).into());
        };
        tree.insert(
            path,
            Node {
                mode: entry.metadata()?.permissions().mode() & 0o7777,
                digest,
            },
        );
    }
    Ok(())
}

fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, Node>> {
    let mut tree = BTreeMap::new();
    visit(root, Path::new(""), &mut tree)?;
    Ok(tree)
}

fn check_inputs(inputs: &Path) -> Result<()> {
    let actual: BTreeMap<_, _> = inventory(inputs)?
        .into_iter()
        .filter_map(|(p, n)| n.digest.map(|d| (p, d)))
        .collect();
    let expected: BTreeMap<_, _> = INPUTS
        .lines()
        .map(|line| {
            let (digest, path) = line.split_once("  ").ok_or("invalid pinned inventory")?;
            Ok::<_, Box<dyn Error>>((PathBuf::from(path), digest.to_owned()))
        })
        .collect::<Result<_>>()?;
    if actual != expected || expected.len() != 111 {
        return Err("input inventory differs from the 111 pinned files".into());
    }
    Ok(())
}

fn compare(base: &Path, candidate: &Path) -> Result<Value> {
    let expected = inventory(base)?;
    let actual = inventory(candidate)?;
    for path in expected.keys().chain(actual.keys()) {
        if expected.get(path) != actual.get(path) {
            return Err(format!("tree mismatch at {}", path.display()).into());
        }
    }
    let lock = fs::read(candidate.join("workspace.lock"))?;
    let members: Value = serde_json::from_slice(&lock)?;
    if sha(&lock) != LOCK || members["members"].as_object().map(|m| m.len()) != Some(7) {
        return Err("workspace lock differs from the seven-member release".into());
    }
    let mut manifest = Vec::new();
    for (path, node) in &expected {
        // NUL is excluded from Unix paths, so tuple boundaries are unambiguous.
        manifest.extend_from_slice(path.as_os_str().as_encoded_bytes());
        manifest.push(0);
        manifest.extend(node.mode.to_be_bytes());
        manifest.extend(node.digest.as_deref().unwrap_or("directory").as_bytes());
        manifest.push(0);
    }
    let files = expected.values().filter(|v| v.digest.is_some()).count();
    Ok(
        json!({"files":files,"directories":expected.len()-files,"treeSha256":sha(&manifest),"workspaceLockSha256":LOCK,"members":7}),
    )
}

fn run(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{command:?}: {status}").into());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<PathBuf> = env::args_os().skip(1).map(Into::into).collect();
    let [
        compiler,
        inputs,
        baseline,
        policy_tool,
        upstream,
        cache,
        receipt,
    ] = args.as_slice()
    else {
        return Err("usage: provider-validation COMPILER PINNED_INPUT_DIR BASELINE_TREE POLICY_VALIDATOR UPSTREAM CARGO_CACHE NEW_RECEIPT".into());
    };
    if receipt.try_exists()? {
        return Err("choose a fresh receipt path".into());
    }
    check_inputs(inputs)?;
    let temp = tempdir()?;
    // Bind the receipt to private copies, including when a build concurrently replaces its CLI.
    let frozen_compiler = temp.path().join("oddlang");
    let frozen_policy = temp.path().join("command-policy-validation");
    fs::copy(compiler, &frozen_compiler)?;
    fs::copy(policy_tool, &frozen_policy)?;
    let workspace = temp.path().join("workspace");
    run(Command::new(&frozen_compiler)
        .args(["build", "native-workspace", "rust"])
        .arg(inputs.join("workspace-command-policy.json"))
        .arg(&workspace))?;
    let tree = compare(baseline, &workspace)?;
    let policy_receipt = temp.path().join("policy.json");
    run(Command::new(&frozen_policy)
        .arg("qualify")
        .arg(upstream)
        .arg(cache)
        .arg(&workspace)
        .arg(&policy_receipt))?;
    let policy: Value = serde_json::from_slice(&fs::read(policy_receipt)?)?;
    for name in ["policy-observer", "generated-observer", "room-observer"] {
        let digest = policy["rebuiltObserverSha256"][name]
            .as_str()
            .ok_or("missing rebuilt observer digest")?;
        if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("invalid observer digest for {name}").into());
        }
    }
    if policy["upstreamCommit"] != "af54339d250290490d62d4475a77539b7e739071"
        || policy["rustToolchain"] != "1.97.1"
        || policy["workspaceLockSha256"] != LOCK
        || policy["host"] != "aarch64-apple-darwin"
        || policy["policyDecisions"]["cases"] != 105
        || policy["roomConfigFileCases"] != 11
        || policy["policyDecisions"]["inputsSha256"]
            != "b392ced173616d91990bb648af275a25776ccc15e22f33faf7388baea451a980"
        || policy["policyDecisions"]["observationsSha256"]
            != "c30e84dc1ec0633fc995d79eda55ece8dfabd27db8cc91bcf72e2270cbbd6f6b"
    {
        return Err("policy replay differs from recorded macOS ARM64 corpus".into());
    }
    let evidence = json!({"compilerSha256":sha(&fs::read(&frozen_compiler)?),"policyToolSha256":sha(&fs::read(&frozen_policy)?),"pinnedInputs":111,
        "inputManifestSha256":sha(INPUTS.as_bytes()),"workspace":tree,"policy":policy});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(receipt)?;
    serde_json::to_writer_pretty(&mut file, &evidence)?;
    std::io::Write::write_all(&mut file, b"\n")?;
    Ok(())
}
