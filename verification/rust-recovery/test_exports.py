"""Qualify type spellings against real dependent-crate name resolution."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import shutil

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent
SOURCE = """
mod hidden {
    pub struct Reading { pub units: u16, pub(crate) calibration: i8, internal: bool }
    #[non_exhaustive] pub struct Incomplete { pub public_field: bool }
    pub enum Payload { Unit, Tuple(u64), Named { item: bool } }
    pub union Bits { pub first: u32, hidden: u8 }
    pub struct Tuple(pub i16, i8);
}
pub use hidden::{Reading as Report, Tuple, Payload, Bits, Incomplete};
pub use hidden::Payload::Named;
pub type ReadingAlias = Report;
pub use std::num::NonZeroU8 as External;
pub use std::fmt::Debug as _;
pub mod outward { pub use super::routes as nested; }
pub use routes as mirror;
pub mod routes {
    pub use super::Report as Alias;
    pub use crate as again;
}
pub mod glob { pub use super::routes::*; }
mod left { pub struct Conflict { pub from_left: u8 } }
mod right { pub struct Conflict { pub from_right: u8 } }
pub mod mixed { pub use super::left::*; pub use super::right::*; }
#[cfg(feature = "extra")]
pub use hidden::Reading as Extra;
"""


def run(cmd, cwd, ok=True):
    result = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)
    if (result.returncode == 0) != ok:
        raise AssertionError(result.stderr)
    return result


with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    package = root / "domain"
    (package / "src").mkdir(parents=True)
    (package / "Cargo.toml").write_text(
        '[package]\nname="observations"\nversion="0.1.0"\nedition="2024"\n[features]\nextra=[]\n'
    )
    src = package / "src/lib.rs"
    src.write_text(SOURCE)
    run(["cargo", "+1.97.1", "generate-lockfile", "--offline"], package)
    host = (
        run(["rustc", "+1.97.1", "-vV"], root).stdout.split("host: ")[1].splitlines()[0]
    )
    profile = root / "profile.toml"

    def recover(name, features=(), project=None):
        active = package if project is None else project
        values = dict(
            adapter=str(HERE / "run.sh"),
            adapterInputs=[
                "Cargo.toml",
                "Cargo.lock",
                "driver.rs",
                "shapes.rs",
                "shape_model.rs",
                "exports.rs",
                "export_model.rs",
            ],
            package="observations",
            targetKind="lib",
            targetName="observations",
            targetTriple=host,
            features=list(features),
            defaultFeatures=False,
            compilerCommit="8bab26f4f68e0e26f0bb7960be334d5b520ea452",
            cargoLockSha256=hashlib.sha256(
                (active / "Cargo.lock").read_bytes()
            ).hexdigest(),
        )
        profile.write_text(
            "".join(f"{k} = {json.dumps(v)}\n" for k, v in values.items())
        )
        out = root / name
        run(
            [
                CLI,
                "recover",
                "rust-exports",
                active / "Cargo.toml",
                "--profile",
                profile,
                "--out",
                out,
            ],
            root,
        )
        data = (out / "exports.json").read_bytes()
        assert (
            hashlib.sha256(data).hexdigest()
            == json.loads((out / "exports.lock").read_bytes())["exportsSha256"]
        )
        return json.loads(data)

    caller = root / "caller"
    (caller / "src").mkdir(parents=True)
    (caller / "Cargo.toml").write_text(
        '[package]\nname="consumer"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nobservations={path="../domain"}\n'
    )

    def probe(declaration, should_compile=True):
        (caller / "src/lib.rs").write_text(declaration)
        return run(
            ["cargo", "+1.97.1", "check", "--offline", "--quiet"],
            caller,
            should_compile,
        )

    graph = recover("baseline")
    repeat = root / "different-source-path"
    shutil.copytree(package, repeat, ignore=shutil.ignore_patterns("target"))
    assert recover("relocated", project=repeat) == graph
    for filename in ("exports.json", "exports.lock"):
        assert (root / "baseline" / filename).read_bytes() == (
            root / "relocated" / filename
        ).read_bytes()
    checked = root / "independent"
    run(
        [
            sys.executable,
            "-B",
            HERE / "probe_exports.py",
            CLI,
            package / "Cargo.toml",
            "observations",
            checked,
        ],
        root,
    )
    independent = json.loads((checked / "evidence.json").read_bytes())
    assert (
        independent["compilePositiveCount"] > 10
        and independent["fieldProbes"]["publicReads"] >= 5
    )
    modules = graph["modules"]
    top = modules[graph["root"]]
    report = top["bindings"]["Report"]["target"]
    assert (
        "hidden" not in top["bindings"]
        and "Extra" not in top["bindings"]
        and "_" not in top["bindings"]
    )
    for spelling in ("routes", "glob"):
        child = modules[top["bindings"][spelling]["target"]]
        assert child["bindings"]["Alias"]["target"] == report
        assert child["bindings"]["again"]["target"] == graph["root"]
    mixed = modules[top["bindings"]["mixed"]["target"]]
    assert "Conflict" in mixed["ambiguous"] and "Conflict" not in mixed["bindings"]
    conflict = mixed["ambiguous"]["Conflict"]
    choices = [conflict["primary"], *conflict["alternatives"]]
    assert {
        choice["binding"]["target"].replace("::{type}", "") for choice in choices
    } == {
        "local::observations::left::Conflict",
        "local::observations::right::Conflict",
    }
    assert all(choice["audience"] == {"kind": "public"} for choice in choices)
    denied = probe(
        "#![deny(ambiguous_glob_imports)]\npub type Invalid = observations::mixed::Conflict;",
        False,
    )
    assert "ambiguous" in denied.stderr and "glob" in denied.stderr
    picked = (
        conflict["primary"]["binding"]["target"].replace("::{type}", "").split("::")[-2]
    )
    probe(f"""#![allow(ambiguous_glob_imports)]
pub fn chosen(value: observations::mixed::Conflict) -> u8 {{ value.from_{picked} }}""")
    assert modules[top["bindings"]["routes"]["target"]]["path"] == ["mirror"]
    assert top["bindings"]["ReadingAlias"]["kind"] == "alias"
    assert top["bindings"]["ReadingAlias"]["target"] != report
    assert top["bindings"]["External"]["kind"] == "external"
    assert top["bindings"]["Named"]["kind"] == "variant"
    fields = graph["data"][report]
    assert fields["fields"]["units"]["audience"] == {"kind": "public"}
    assert fields["fields"]["calibration"]["audience"] == {
        "kind": "scope",
        "id": graph["root"],
    }
    diagnostic = probe(
        "pub fn inaccessible(r: observations::Report) -> i8 { r.calibration }", False
    )
    assert "E0616" in diagnostic.stderr
    diagnostic = probe("pub type Private = observations::hidden::Reading;", False)
    assert "E0603" in diagnostic.stderr
    probe("pub use observations::mirror::again as cycle_edge;")
    probe("pub use observations::outward::nested as alias_edge;")
    probe(
        "pub fn named(v: observations::Payload) { if let observations::Payload::Named {item} = v { let _ = item; } }"
    )
    probe(
        "pub fn tuple(v: observations::Payload) { if let observations::Payload::Tuple(item) = v { let _ = item; } }"
    )
    probe("pub fn bits(v: observations::Bits) -> u32 { unsafe {v.first} }")
    assert (
        "non-exhaustive"
        in probe(
            "pub fn construct() -> observations::Incomplete { observations::Incomplete {public_field: true} }",
            False,
        ).stderr
    )
    assert (
        "E0423"
        in probe(
            "pub fn construct() -> observations::Tuple { observations::Tuple(0,0) }",
            False,
        ).stderr
    )
    probe("pub type Private = observations::hidden::Reading;", False)
    probe("pub fn first(r: observations::Tuple) -> i16 {r.0}")
    probe("pub fn second(r: observations::Tuple) -> i8 {r.1}", False)
    src.write_text(SOURCE.replace("Report", "Summary"))
    changed = recover("changed", ["extra"])
    top2 = changed["modules"][changed["root"]]
    assert (
        "Report" not in top2["bindings"]
        and top2["bindings"]["Summary"]["target"] == report
    )
    assert top2["bindings"]["Extra"]["target"] == report
    probe("pub fn new(r: observations::Summary) -> u16 {r.units}")
    assert "E0425" in probe("pub type Old = observations::Report;", False).stderr
    (caller / "Cargo.toml").write_text(
        (caller / "Cargo.toml")
        .read_text()
        .replace('path="../domain"', 'path="../domain", features=["extra"]')
    )
    probe("pub use observations::Extra as independently_selected;")
    src.write_text("""
#![deny(ambiguous_import_visibilities)]
pub mod museum {
    mod catalog { pub struct Entry; }
    pub use catalog::*;
    macro_rules! shadow { () => { use catalog::Entry; } }
    shadow!();
    pub use Entry as Renamed;
}
""")
    hidden_collision = run(
        ["cargo", "+1.97.1", "check", "--locked", "--offline"], package, False
    )
    assert "ambiguous_import_visibilities" in hidden_collision.stderr
    print(
        "checked aliases, public module cycles, visibility and changed export names against native callers"
    )
