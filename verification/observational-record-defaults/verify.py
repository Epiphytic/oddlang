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

    serialized = "use serde::Serialize;\n" + SOURCE.replace("Debug, Default", "Debug, Default, Serialize")
    recover(root / "serialized-path", serialized, expected="serialized public path field has no portable wire rule")
    original = root / "baseline" / "recovered"
    graph = json.loads((original / "graph.json").read_bytes())
    graph["modules"][0]["records"][0]["traits"].append("serialize")
    (root / "serialized.json").write_text(json.dumps(graph))
    run(CLI, "build", "public-records", "rust", root / "serialized.json",
        original / "rust.json", root / "invalid-wire", cwd=root,
        expected="serialized public path field has no portable wire rule")

    shadowed = SOURCE.replace("struct $name;", "#[derive(Clone, Debug, Default)] struct $name;")
    shadowed = shadowed.replace("unit!(Hidden);", "unit!(u32);")
    recover(root / "macro-shadow", shadowed)
    work = root / "macro-shadow"
    assert "macro expansion" in json.loads((work / "recovered/recovery.lock").read_bytes())["gap"]
    nominal_width = "fn main() { let _: ::core::primitive::u32 = CartographicSample::default().waypoints; }"
    (work / "native.rs").write_text(shadowed + nominal_width)
    run("rustc", "--edition=2021", "native.rs", cwd=work, expected="mismatched types")

    extra = SOURCE + """
macro_rules! attach_equality {
    () => {
        impl PartialEq for CartographicSample {
            fn eq(&self, other: &Self) -> bool { self.waypoints == other.waypoints }
        }
    };
}
attach_equality!();
"""
    recover(root / "macro-impl", extra)
    work = root / "macro-impl"
    comparison = "fn main() { assert_eq!(CartographicSample::default(), CartographicSample::default()); }"
    (work / "native.rs").write_text(extra + comparison)
    run("rustc", "--edition=2021", "native.rs", cwd=work)
    run(CLI, "build", "public-records", "rust", "recovered/graph.json", "recovered/rust.json",
        "projection", cwd=work)
    (work / "projected.rs").write_text('include!("projection/records.rs");\n' + comparison)
    run("rustc", "--edition=2021", "projected.rs", cwd=work, expected="binary operation")
    print("Unsigned fields, fieldwise defaults, native paths and unresolved macro controls passed.")
