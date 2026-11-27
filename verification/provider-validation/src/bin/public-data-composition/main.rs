use clap::{Parser, Subcommand};
use provider_validation::{Result, read, sha};
use serde_json::{Value, json};
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
mod witnesses;
use witnesses::{COMPOSITION, INHERITED};

const RUNTIME: &str = "900cfa517d0136fa80c2b5e92ccf9b1a3b2a71ab";
const PREDECESSOR: &str = "verification/structural-profile-composition/aarch64-apple-darwin.json";
const PREDECESSOR_SHA: &str = "7eeacdf29785d78a7677b9793b43e0178bebb1e01420794b9097f2b40e889a7b";
const ARCHIVES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../public-data-composition");
const DARWIN: &str = "aarch64-apple-darwin";
const FORMAT: &str = "oddlang-public-data-composition-replay-v1";

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    mode: Mode,
}
#[derive(Subcommand)]
enum Mode {
    Verify {
        root: PathBuf,
        target: PathBuf,
        receipt: PathBuf,
        #[arg(long)]
        with_node: bool,
        /// Replay an existing executable whose digest matches the host's frozen composition receipt.
        #[arg(long)]
        binary: Option<PathBuf>,
    },
    Inherited {
        root: PathBuf,
        binary: PathBuf,
        receipt: PathBuf,
        result: PathBuf,
    },
}
fn require(condition: bool, message: &str) -> Result {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(format!(
            "{cmd:?}: {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}
fn clean(root: &Path) -> Result {
    let rev = output(
        Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"]),
    )?;
    let status = output(Command::new("git").current_dir(root).args([
        "status",
        "--porcelain",
        "--untracked-files=all",
    ]))?;
    require(
        rev == RUNTIME && status.is_empty(),
        "expected the clean pinned compiler checkout",
    )?;
    let top = output(
        Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "--show-toplevel"]),
    )?;
    require(
        Path::new(&top).canonicalize()? == root,
        "expected the pinned checkout root",
    )
}
fn archived(host: &str, inherited: bool) -> Result<Option<(Value, &'static str)>> {
    let digest = match (host, inherited) {
        ("aarch64-apple-darwin", false) => {
            "45ebf0e8a04c503874ecc418ddadcb8428eebec9dc2403e03ebcbd2061a2257b"
        }
        ("aarch64-apple-darwin", true) => {
            "110a721118b1e7e3036df6aae948900f2bc1968b48ffe26751b5cbc1b45d454e"
        }
        ("aarch64-unknown-linux-gnu", false) => {
            "0040278cdac4f18b1e16e7a580f53a975c1ca8a428853b4c810d1fd7f03fca23"
        }
        ("aarch64-unknown-linux-gnu", true) => {
            "708683ac06c51c956a073f07a56fae923e84719bc13d8314dedf4a7a3ccac770"
        }
        _ => return Ok(None),
    };
    let prefix = if inherited { "inherited-" } else { "" };
    let path = Path::new(ARCHIVES).join(format!("{prefix}{host}.json"));
    require(sha(&path)? == digest, "frozen receipt digest mismatch")?;
    Ok(Some((read(&path)?, digest)))
}
fn pathsafe(path: &Path) -> Result<PathBuf> {
    let abs = std::path::absolute(path)?;
    if abs.exists() {
        return Ok(abs.canonicalize()?);
    }
    let name = abs.file_name().ok_or("missing output filename")?;
    Ok(pathsafe(abs.parent().ok_or("missing output directory")?)?.join(name))
}
fn publish(path: &Path, value: &Value, root: &Path, protected: &[&Path]) -> Result {
    let dst = pathsafe(path)?;
    require(
        !dst.starts_with(root)
            && !dst.starts_with(Path::new(ARCHIVES).canonicalize()?)
            && !protected.contains(&dst.as_path()),
        "receipt destination would overwrite a protected input",
    )?;
    let mut file = tempfile::NamedTempFile::new_in(dst.parent().ok_or("missing receipt parent")?)?;
    serde_json::to_writer_pretty(file.as_file_mut(), value)?;
    file.write_all(b"\n")?;
    file.persist(dst)?;
    Ok(())
}
fn rust_version(tool: &str) -> Result<Vec<String>> {
    Ok(output(Command::new("rustc").args([tool, "-vV"]))?
        .lines()
        .map(str::to_owned)
        .collect())
}
struct Context {
    root: PathBuf,
    host: String,
    fixture_rust: Vec<String>,
    predecessor: Value,
    tests: Vec<String>,
}
impl Context {
    fn open(root: PathBuf) -> Result<Self> {
        let root = root.canonicalize()?;
        clean(&root)?;
        let file = root.join(PREDECESSOR);
        require(
            sha(&file)? == PREDECESSOR_SHA,
            "predecessor inventory differs from the pinned receipt",
        )?;
        let predecessor = read(&file)?;
        let mut tests: Vec<String> = predecessor["cargoTests"]["lib"]
            .as_array()
            .ok_or("missing predecessor tests")?
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| s.starts_with("test ") && s.ends_with(" ... ok"))
            .map(str::to_owned)
            .collect();
        tests.sort();
        require(!tests.is_empty(), "empty predecessor test set")?;
        let fixture_rust = rust_version("+1.97.1")?;
        let host = fixture_rust
            .iter()
            .find_map(|s| s.strip_prefix("host: "))
            .ok_or("missing rustc host")?
            .to_owned();
        Ok(Self {
            root,
            host,
            fixture_rust,
            predecessor,
            tests,
        })
    }
    fn cargo(&self, action: &str, target: &Path, flags: &[&str]) -> Result<String> {
        output(
            Command::new("cargo")
                .current_dir(&self.root)
                .env_remove("CARGO_TARGET_DIR")
                .args(["+1.96.1", action, "--offline", "--locked", "--target-dir"])
                .arg(target)
                .args(flags),
        )
    }
    fn finish(&self, cli: &Path, digest: &str) -> Result {
        clean(&self.root)?;
        require(sha(cli)? == digest, "compiler changed during qualification")
    }
}

fn verify(
    c: &Context,
    target: &Path,
    out: &Path,
    with_node: bool,
    binary: Option<&Path>,
) -> Result {
    let target = pathsafe(target)?;
    require(!target.starts_with(&c.root), "use an external Cargo target")?;
    let frozen = archived(&c.host, false)?;
    let archive_digest = frozen.as_ref().map(|(_, d)| *d);
    let (cli, origin) = if let Some(bin) = binary {
        let cli = bin.canonicalize()?;
        let (archive, _) = frozen
            .as_ref()
            .ok_or("no frozen compiler receipt for this host")?;
        require(
            archive["binarySha256"] == sha(&cli)?,
            "compiler differs from the frozen composition receipt",
        )?;
        (cli, "frozen-host-executable")
    } else {
        c.cargo("build", &target, &["--bin", "oddlang"])?;
        (
            target.join("debug/oddlang").canonicalize()?,
            "built-from-pinned-checkout",
        )
    };
    let digest = sha(&cli)?;
    let (all_cases, darwin_digest) = archived(DARWIN, false)?.unwrap();
    let cases = if with_node {
        &COMPOSITION[..]
    } else {
        &COMPOSITION[..5]
    };
    let results = witnesses::run(&c.root, &cli, cases, &all_cases, false)?;
    let tests = c.cargo("test", &target, &["--lib"])?;
    let mut passed: Vec<_> = tests
        .lines()
        .filter(|s| s.starts_with("test ") && s.ends_with(" ... ok"))
        .collect();
    passed.sort();
    require(
        passed == c.tests,
        "library test names differ from the pinned predecessor",
    )?;
    let mut receipt = json!({"format":FORMAT,"runtime":RUNTIME,"host":c.host,"binarySha256":digest,"binaryOrigin":origin,
        "matchesFrozenHostExecutable":frozen.as_ref().map(|(f,_)|f["binarySha256"]==digest),
        "frozenHostCompositionReceiptSha256":archive_digest,"historicalFixtureInventorySha256":darwin_digest,
        "fixtureRustc":c.fixture_rust,"compilerRustc":rust_version("+1.96.1")?,"runnerSha256":sha(&env::current_exe()?)?,
        "fixtures":results,"predecessorTestReceiptSha256":PREDECESSOR_SHA,"libraryTests":passed});
    if with_node {
        receipt["node"] = output(Command::new("node").arg("--version"))?.into();
        receipt["typescript"] = output(Command::new("npx").args([
            "--offline",
            "--package=typescript@5.9.3",
            "tsc",
            "--version",
        ]))?
        .into();
    }
    c.finish(&cli, &digest)?;
    publish(out, &receipt, &c.root, &[&cli])
}
fn parent_pins(c: &Context, input: &Value, parent_digest: &str, digest: &str) -> Result {
    require(
        input["runtime"] == RUNTIME && input["binarySha256"] == digest,
        "compiler differs from the native composition receipt",
    )?;
    require(
        input["host"] == c.host,
        "composition receipt host differs from native host",
    )?;
    require(
        input["predecessorTestReceiptSha256"] == PREDECESSOR_SHA
            && input["libraryTests"] == json!(c.tests),
        "composition receipt predecessor or test inventory mismatch",
    )?;
    let frozen = archived(&c.host, false)?;
    if input.get("format").is_none() {
        require(
            frozen
                .as_ref()
                .is_some_and(|(f, d)| *d == parent_digest && f["binarySha256"] == digest),
            "unrecognized frozen composition receipt",
        )?;
    } else {
        require(
            input["format"] == FORMAT,
            "unsupported composition receipt format",
        )?;
        require(
            input["frozenHostCompositionReceiptSha256"] == json!(frozen.as_ref().map(|(_, d)| d)),
            "composition receipt host archive pin mismatch",
        )?;
        witnesses::check_composition_inventory(input)?;
        require(
            input["matchesFrozenHostExecutable"]
                == json!(frozen.as_ref().map(|(f, _)| f["binarySha256"] == digest)),
            "composition executable comparison differs from host archive",
        )?;
        match input["binaryOrigin"].as_str() {
            Some("frozen-host-executable") => require(
                frozen
                    .as_ref()
                    .is_some_and(|(f, _)| f["binarySha256"] == digest),
                "compiler differs from the frozen composition receipt",
            )?,
            Some("built-from-pinned-checkout") => (),
            _ => return Err("unknown composition compiler origin".into()),
        }
    }
    Ok(())
}
fn inherited(c: &Context, binary: &Path, parent: &Path, out: &Path) -> Result {
    let cli = binary.canonicalize()?;
    let parent = parent.canonicalize()?;
    let digest = sha(&cli)?;
    let parent_digest = sha(&parent)?;
    let input = read(&parent)?;
    parent_pins(c, &input, &parent_digest, &digest)?;
    let mut paths: Vec<_> = c.predecessor["fixtures"]
        .as_array()
        .ok_or("missing predecessor inventory")?
        .iter()
        .map(|e| e["fixture"].as_str())
        .collect();
    paths.push(Some("verification/plain-value-choices/verify.py"));
    require(
        paths
            .iter()
            .copied()
            .eq(INHERITED.iter().map(|w| Some(w.legacy))),
        "inherited fixture inventory differs from predecessor",
    )?;
    let (archive, anchor) = match archived(&c.host, true)? {
        Some(a) => a,
        None => archived(DARWIN, true)?.unwrap(),
    };
    require(
        archive["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str())
            .eq(paths),
        "archived inherited fixture order differs from predecessor",
    )?;
    let rows = witnesses::run(&c.root, &cli, &INHERITED, &archive, true)?;
    let result = json!({"format":"oddlang-public-data-inherited-replay-v1","runtime":RUNTIME,"host":c.host,"binarySha256":digest,
        "compositionReceiptSha256":parent_digest,"historicalFixtureInventorySha256":anchor,"runnerSha256":sha(&env::current_exe()?)?,
        "predecessorTestReceiptSha256":PREDECESSOR_SHA,"fixtures":rows});
    c.finish(&cli, &digest)?;
    require(
        sha(&parent)? == parent_digest,
        "composition receipt changed during replay",
    )?;
    publish(out, &result, &c.root, &[&cli, &parent])
}
fn main() -> Result {
    match Args::parse().mode {
        Mode::Verify {
            root,
            target,
            receipt,
            with_node,
            binary,
        } => verify(
            &Context::open(root)?,
            &target,
            &receipt,
            with_node,
            binary.as_deref(),
        ),
        Mode::Inherited {
            root,
            binary,
            receipt,
            result,
        } => inherited(&Context::open(root)?, &binary, &receipt, &result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archived_parent_binds_exact_receipt_bytes_runtime_executable_host_and_predecessor() -> Result
    {
        let (receipt, pin) = archived(DARWIN, false)?.unwrap();
        let digest = receipt["binarySha256"].as_str().unwrap();
        let tests = serde_json::from_value(receipt["libraryTests"].clone())?;
        let c = Context {
            root: PathBuf::new(),
            host: DARWIN.into(),
            fixture_rust: Vec::new(),
            predecessor: Value::Null,
            tests,
        };
        parent_pins(&c, &receipt, pin, digest)?;
        assert!(parent_pins(&c, &receipt, "different receipt", digest).is_err());
        assert!(parent_pins(&c, &receipt, pin, "different binary").is_err());
        for (field, change) in [
            ("runtime", json!("other")),
            ("host", json!("other")),
            ("predecessorTestReceiptSha256", json!("other")),
            ("libraryTests", json!([])),
        ] {
            let mut altered = receipt.clone();
            altered[field] = change;
            assert!(parent_pins(&c, &altered, pin, digest).is_err(), "{field}");
        }
        Ok(())
    }
}
