"""Observe a recovered typed error, finite choice and named product."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import copy
import tempfile

CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1")
SOURCE = '''use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentReason { Missing, Omitted }
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum ArchiveFault {
    #[error("archive unavailable")]
    Unavailable,
    #[error("document rejected: {0:?}")]
    Rejected(DocumentReason),
    #[error("reader full")]
    Full { first_byte: u64, capacity: u32 }
}
pub type Outcome<T> = std::result::Result<T, ArchiveFault>;
'''

PROBE = '''
include!("projection/errors.rs");

fn discriminant(reason: DocumentReason) -> u8 {
    match reason {
        DocumentReason::Missing => 0,
        DocumentReason::Omitted => 1,
    }
}

fn main() {
    let reason = DocumentReason::Omitted;
    assert_eq!(reason.clone(), reason);
    assert_eq!(reason as u8, discriminant(reason));
    assert_eq!(format!("{reason:?}"), "Omitted");
    let absent: Outcome<()> = Err(ArchiveFault::Rejected(reason));
    let rejected = absent.unwrap_err();
    assert_eq!(rejected, ArchiveFault::Rejected(reason));
    assert_eq!(rejected.to_string(), "document rejected: Omitted");
    assert_eq!(format!("{rejected:?}"), "Rejected(Omitted)");
    let full = ArchiveFault::Full { first_byte: u64::MAX, capacity: u32::MAX };
    assert!(::std::error::Error::source(&full).is_none());
    match full {
        ArchiveFault::Unavailable | ArchiveFault::Rejected(_) => panic!("wrong value"),
        ArchiveFault::Full { first_byte, capacity } => {
            assert_eq!(first_byte, u64::MAX);
            assert_eq!(capacity, u32::MAX);
        }
    }
    println!("{}", ArchiveFault::Unavailable);
}
'''


def run(*command, cwd, expected=None):
    result = subprocess.run(command, cwd=cwd, env=ENV, capture_output=True, text=True)
    if expected is not None:
        assert result.returncode and expected in result.stderr, result
    elif result.returncode:
        raise ValueError(result.stderr)
    return result.stdout


def recover(root, source, *, expected=None):
    (root / "selected.rs").write_text(source)
    (root / "selection.json").write_text(json.dumps({
        "format": "oddlang-typed-finite-error-selection-v1",
        "id": "urn:example:archive",
        "sourceSha256": hashlib.sha256(source.encode()).hexdigest(),
        "error": "ArchiveFault", "result": "Outcome",
        "choices": ["DocumentReason"], "variantIds": {"Unavailable": "urn:example:availability"},
    }))
    run(CLI, "recover", "finite-error-domain", "selected.rs", "selection.json", "recovered",
        cwd=root, expected=expected)


def process_link(root):
    (root / "Cargo.lock").write_text('version = 4\n[[package]]\nname = "typed-archive"\nversion = "0.1.0"\n')

    def pin(relative, value=None):
        path = root / relative
        if value is not None:
            path.write_text(json.dumps(value))
        return {"path": relative, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    failure = "urn:example:availability"
    errors = dict(kind="finite-error-domain", module="types",
                  reexports=["ArchiveFault", "Outcome", "DocumentReason"],
                  graph=pin("recovered/graph.json"), overlay=pin("recovered/rust.json"))
    selector = {"entry": "initialize_budget", "error": {
        "id": failure, "typePath": ["ArchiveFault"], "variant": "Unavailable",
    }}
    process = dict(kind="process-limit", module="limits", reexports=["initialize_budget"], graph=pin(
        "policy.json", {"format": "oddlang-process-limit-v1", "id": "urn:example:policy",
                        "failure": failure, "resource": "open-descriptors",
                        "platforms": {"linux": {"kind": "no-op"}, "macos": {"kind": "no-op"}}}),
        overlay=pin("policy-rust.json", selector))
    package = dict(format="oddlang-native-package-v1", name="typed-archive", version="0.1.0",
                   edition="2021", cargoLock=pin("Cargo.lock"), dependencies={}, components=[errors, process])
    pin("package.json", package)
    run(CLI, "build", "native-package", "rust", "package.json", "product", cwd=root)
    probe = root / "product/src/bin/test.rs"
    probe.parent.mkdir()
    probe.write_text("""
use typed_archive::{initialize_budget, ArchiveFault, Outcome};
fn main() {
    let initialize: fn() -> Outcome<()> = initialize_budget;
    assert!(initialize().is_ok());
    assert_eq!(ArchiveFault::Unavailable.to_string(), "archive unavailable");
}
""")
    run("cargo", "run", "--locked", "--offline", "--bin", "test", cwd=root / "product")
    for suffix, components, variant in [
        ("absent", [process], "Unavailable"),
        ("misbound", [errors, process], "Rejected"),
    ]:
        negative = copy.deepcopy(package)
        negative["components"] = copy.deepcopy(components)
        selector["error"]["variant"] = variant
        negative["components"][-1]["overlay"] = pin("negative-rust.json", selector)
        pin("negative.json", negative)
        run(CLI, "build", "native-package", "rust", "negative.json", "negative-" + suffix,
            cwd=root, expected="unresolved declared process limit error")
    

with tempfile.TemporaryDirectory(prefix="odd-typed-error-") as directory:
    root = Path(directory)
    for name in ("baseline", "changed", "unsupported"):
        (root / name).mkdir()
    for kind, source, message in [
        ("baseline", SOURCE, "archive unavailable"),
        ("changed", SOURCE.replace("archive unavailable", "archive closed"), "archive closed"),
    ]:
        work = root / kind
        recover(work, source)
        run(CLI, "build", "finite-error-domain", "rust", "recovered/graph.json",
            "recovered/rust.json", "projection", cwd=work)
        (work / "probe.rs").write_text(PROBE)
        run("rustc", "--edition=2021", "probe.rs", cwd=work)
        assert run(work / "probe", cwd=work) == message + "\n"
    process_link(root / "baseline")
    recover(root / "unsupported", SOURCE.replace("capacity: u32", "capacity: String"),
            expected="unsupported typed error payload field")
    for index, shadow in enumerate([
        "pub enum u32 { Case }", "pub union u32 { byte: u8 }",
        "pub trait u32 {}", "pub enum Debug { Case }",
    ]):
        work = root / f"shadow-{index}"
        work.mkdir()
        recover(work, SOURCE + "\n" + shadow, expected="shadowing typed error declaration")
    print("Unrelated typed enum, fixed-width product and diagnostic mutation passed.")
