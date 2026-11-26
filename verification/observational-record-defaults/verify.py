"""Verify fieldwise defaults, fixed-width values and paths through native Rust."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1")
SOURCE = """use std::path::PathBuf;
macro_rules! unit { ($name:ident) => {struct $name;} }
unit!(Hidden);

#[derive(Clone, Debug, Default)]
pub struct CartographicSample {
    pub waypoints: u32,
    pub points: u64,
    pub current: bool,
    pub reference: PathBuf,
}
"""
SELECT = {"CartographicSample": "urn:example:cartography:sample"}
PROBE = """include!("projection/records.rs");
fn main() {
    let mut initial = CartographicSample::default();
    let _: (u32, u64, bool, std::path::PathBuf) =
        (initial.waypoints, initial.points, initial.current, initial.reference.clone());
    assert_eq!(initial.waypoints, 0);
    assert_eq!(initial.points, 0);
    assert!(!initial.current);
    assert!(initial.reference.as_os_str().is_empty());
    initial.waypoints = u32::MAX;
    initial.points = u64::MAX;
    initial.reference = "../unknown".into();
    let copied = initial.clone();
    initial.reference.push("child");
    assert_eq!(copied.reference, std::path::Path::new("../unknown"));
    println!("{copied:?}");
}
"""


def run(*argv, cwd, expected=None):
    proc = subprocess.run(argv, cwd=cwd, env=ENV, capture_output=True, text=True)
    if expected is not None:
        assert proc.returncode and expected in proc.stderr, proc
    elif proc.returncode:
        raise RuntimeError(proc.stderr)
    return proc.stdout


def recover(root, source, *, expected=None):
    root.mkdir()
    (root / "input.rs").write_text(source)
    (root / "selection.json").write_text(json.dumps({
        "format": "oddlang-rust-selected-record-selection-v1",
        "id": "urn:example:cartography", "sourceSha256": hashlib.sha256(source.encode()).hexdigest(),
        "records": SELECT,
    }))
    run(CLI, "recover", "public-root-records", "input.rs", "selection.json", "recovered",
        cwd=root, expected=expected)


with tempfile.TemporaryDirectory(prefix="odd-record-defaults-") as directory:
    root = Path(directory)
    for label, source in [
        ("baseline", SOURCE),
        ("changed", SOURCE.replace("    pub waypoints: u32,\n    pub points: u64,",
                                   "    pub points: u64,\n    pub waypoints: u32,")),
    ]:
        work = root / label
        recover(work, source)
        run(CLI, "build", "public-records", "rust", "recovered/graph.json", "recovered/rust.json",
            "projection", cwd=work)
        (work / "probe.rs").write_text(PROBE)
        run("rustc", "--edition=2021", "probe.rs", cwd=work)
        got = run(work / "probe", cwd=work)
        assert "current: false" in got and 'reference: "../unknown"' in got
        assert (got.index("waypoints:") < got.index("points:")) == (label == "baseline")
    recover(root / "shadow", SOURCE + "\npub enum u32 { Example }", expected="shadowed public record")
    recover(root / "default-shadow", SOURCE + "\nuse core::clone::Clone as Default;",
            expected="shadowed public record")
    recover(root / "missing-path", SOURCE.replace("use std::path::PathBuf;", ""),
            expected="unresolved public path field")
    recover(root / "extra-impl", SOURCE + "\nimpl CartographicSample { pub fn extra() {} }",
            expected="unsupported macros, aliases or impls")
    print("Unsigned record fields, defaults and path values matched through native Rust.")
