use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(super) struct Witness {
    pub legacy: &'static str,
    binary: &'static str,
    case: &'static str,
}
const fn witness(legacy: &'static str, binary: &'static str, case: &'static str) -> Witness {
    Witness {
        legacy,
        binary,
        case,
    }
}
pub(super) const COMPOSITION: [Witness; 6] = [
    witness(
        "verification/observational-record-defaults/pointer_width.py",
        "record-pointer-width",
        "",
    ),
    witness(
        "verification/plain-value-choices/capabilities.py",
        "qualify-choices",
        "capabilities",
    ),
    witness(
        "verification/shared-declarations/verify.py",
        "shared-declarations",
        "",
    ),
    witness(
        "verification/serde-string-choice/verify.py",
        "qualify-choices",
        "serde-string",
    ),
    witness(
        "verification/plain-value-choices/typescript_subset.py",
        "qualify-choices",
        "typescript-subset",
    ),
    witness(
        "verification/plain-value-choices/typescript.py",
        "qualify-choices",
        "typescript",
    ),
];
pub(super) const INHERITED: [Witness; 9] = [
    witness(
        "verification/line-grammar/verify_records.py",
        "line-grammar",
        "records",
    ),
    witness(
        "verification/line-grammar/verify_support.py",
        "line-grammar",
        "support",
    ),
    witness(
        "verification/line-grammar/verify.py",
        "line-grammar",
        "inventory",
    ),
    witness(
        "verification/observational-record-defaults/verify.py",
        "record-defaults",
        "",
    ),
    witness(
        "verification/plain-value-choices/equality.py",
        "qualify-choices",
        "equality",
    ),
    witness(
        "verification/structured-diagnostics/check.py",
        "structured-diagnostics",
        "",
    ),
    witness(
        "verification/command-surface/prefix.py",
        "command-surface",
        "prefix",
    ),
    witness(
        "verification/typed-finite-errors/verify.py",
        "typed-finite-errors",
        "",
    ),
    witness(
        "verification/plain-value-choices/verify.py",
        "qualify-choices",
        "verify",
    ),
];
fn executables(cases: &[Witness]) -> Result<BTreeMap<String, PathBuf>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let names: BTreeSet<_> = cases.iter().map(|w| w.binary).collect();
    let mut cmd = Command::new("cargo");
    cmd.current_dir(manifest)
        .args([
            "+1.97.1",
            "build",
            "--locked",
            "--offline",
            "--message-format=json",
            "--manifest-path",
        ])
        .arg(manifest.join("Cargo.toml"));
    for name in &names {
        cmd.args(["--bin", name]);
    }
    let mut bins = BTreeMap::new();
    for line in output(&mut cmd)?.lines() {
        let artifact: Value = serde_json::from_str(line)?;
        if artifact["reason"] == "compiler-artifact"
            && let (Some(name), Some(path)) = (
                artifact["target"]["name"].as_str(),
                artifact["executable"].as_str(),
            )
            && names.contains(name)
        {
            bins.insert(name.to_owned(), PathBuf::from(path));
        }
    }
    if bins.len() != names.len() {
        return Err("Cargo did not report every fixture executable".into());
    }
    Ok(bins)
}
// Include runtime file inputs alongside the binary digest (embedded fixtures are covered by the binary).
fn file_inputs(binary: &str) -> Result<BTreeMap<&'static str, String>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files: &[&str] = match binary {
        "shared-declarations" => &[
            "verification/shared-declarations/graph.json",
            "verification/shared-declarations/binding.json",
            "tests/fixtures/json-envelope/Cargo.lock",
        ],
        "line-grammar" => &[
            "verification/line-grammar/inventory.json",
            "verification/line-grammar/inventory-rejections.json",
            "verification/line-grammar/inventory.rs",
            "verification/line-grammar/extras.rs",
            "verification/line-grammar/support.json",
            "verification/line-grammar/support-rejections.json",
            "verification/line-grammar/support.rs",
            "verification/line-grammar/records.json",
            "verification/line-grammar/records-rejections.json",
            "verification/line-grammar/records.rs",
        ],
        "structured-diagnostics" => &[
            "verification/structured-diagnostics/Cargo.toml",
            "verification/structured-diagnostics/Cargo.lock",
            "verification/structured-diagnostics/caller.rs",
            "verification/structured-diagnostics/reference.rs",
        ],
        "command-surface" => &[
            "verification/command-surface/Cargo.toml",
            "verification/command-surface/Cargo.lock",
            "verification/command-surface/prefix-reference.rs",
            "verification/command-surface/prefix-cases.json",
        ],
        _ => &[],
    };
    files
        .iter()
        .map(|f| Ok((*f, sha(&root.join(f))?)))
        .collect()
}
pub(super) fn run(
    root: &Path,
    cli: &Path,
    cases: &[Witness],
    archive: &Value,
    inherited: bool,
) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    // Check all historical fixture pins before launching the first current fixture.
    for w in cases {
        let original = if inherited {
            archive["fixtures"]
                .as_array()
                .ok_or("missing inherited fixtures")?
                .iter()
                .find(|r| r["path"] == w.legacy)
                .ok_or("missing inherited fixture")?
        } else {
            archive["fixtures"]
                .get(w.legacy)
                .ok_or("missing composition fixture")?
        };
        let old_sha = sha(&root.join(w.legacy))?;
        require(
            original["fixtureSha256"] == old_sha,
            "historical fixture digest mismatch",
        )?;
        rows.push(json!({"historicPath":w.legacy,"historicFixtureSha256":old_sha,
            "historicStdout":original[if inherited{"stdout"}else{"output"}].as_str().ok_or("missing historical fixture stdout")?}));
    }
    let bins = executables(cases)?;
    for (w, row) in cases.iter().zip(&mut rows) {
        let path = &bins[w.binary];
        let initial = sha(path)?;
        let inputs = file_inputs(w.binary)?;
        let mut cmd = Command::new(path);
        cmd.current_dir(root)
            .env_remove("CARGO_TARGET_DIR")
            .env("RUSTUP_TOOLCHAIN", "1.97.1")
            .env("CARGO_NET_OFFLINE", "true");
        if !w.case.is_empty() {
            cmd.arg(w.case);
        }
        let stdout = output(cmd.arg(cli))?;
        require(
            sha(path)? == initial && inputs == file_inputs(w.binary)?,
            "Rust fixture or its inputs changed during replay",
        )?;
        row["rustFixture"] = json!({"binary":w.binary,"arguments":if w.case.is_empty(){vec![]}else{vec![w.case]},"binarySha256":initial,
            "runtimeInputs":inputs,"stdout":stdout,"exit":0});
    }
    Ok(rows)
}

pub(super) fn check_composition_inventory(parent: &Value) -> Result {
    let (archive, pin) = archived(DARWIN, false)?.unwrap();
    require(
        parent["historicalFixtureInventorySha256"] == pin,
        "composition fixture inventory pin mismatch",
    )?;
    let rows = parent["fixtures"]
        .as_array()
        .ok_or("missing composition fixtures")?;
    let count = if parent.get("typescript").is_some() {
        6
    } else {
        5
    };
    require(rows.len() == count, "missing composition fixture results")?;
    for (w, row) in COMPOSITION.iter().zip(rows) {
        let historic = &archive["fixtures"][w.legacy];
        let runner = &row["rustFixture"];
        let arguments = if w.case.is_empty() {
            json!([])
        } else {
            json!([w.case])
        };
        require(
            row["historicPath"] == w.legacy
                && row["historicFixtureSha256"] == historic["fixtureSha256"]
                && row["historicStdout"] == historic["output"]
                && runner["binary"] == w.binary
                && runner["arguments"] == arguments
                && runner["exit"] == 0
                && runner["binarySha256"]
                    .as_str()
                    .is_some_and(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit())),
            "composition fixture result differs from its pinned inventory",
        )?;
    }
    Ok(())
}
