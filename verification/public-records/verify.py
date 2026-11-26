"""Recover a source mutation and compare independent native record consumers."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1", CARGO_NET_OFFLINE="true")
ROOT = "urn:test:irrigation"
DECLARATION = """use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct Reading {
    pub label: String,
    pub epoch: u64,
    pub flags: Vec<u8>,
    pub active: bool,
}
pub fn unrelated() -> &'static str { "must not enter the projection" }
"""
ROOT_SOURCE = "pub mod sampling;\npub mod exports;\n"
EXPORT = "pub use crate::sampling::{Reading as Archived};\n"
CONSUMER = """
use indicator_core::{sampling::Reading, exports::Archived};
fn map_to_archive(item: Reading) -> Archived { item }
fn map_to_reading(item: Archived) -> Reading { item }
fn main() {
    let mut baseline: Archived = map_to_archive(Reading {
        label: "East\\nDrain".into(), epoch: u64::MAX, flags: vec![0,255], active: false,
    });
    let independent: Reading = map_to_reading(baseline.clone());
    baseline.flags[0] = 1;
    let _: &String = &independent.label;
    let _: u64 = independent.epoch;
    let _: &Vec<u8> = &independent.flags;
    let _: bool = independent.active;
    assert_eq!(independent.flags, vec![0,255]);
    println!("{}", serde_json::to_string(&independent).unwrap());
}
"""


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pin(root, path):
    return {"path": path.relative_to(root).as_posix(), "sha256": sha(path)}


def run(*args, cwd, error=None):
    completed = subprocess.run(args, cwd=cwd, env=ENV, capture_output=True, timeout=240)
    if error:
        assert completed.returncode and error in completed.stderr.decode(), completed
    else:
        assert not completed.returncode, completed.stderr.decode()
    return completed.stdout


def write_json(path, value):
    path.write_text(json.dumps(value))
    return path


with tempfile.TemporaryDirectory() as directory:
    work = Path(directory)
    source = work / "reference"
    (source/"src").mkdir(parents=True)
    root = source/"src/lib.rs"
    records = source/"src/sampling.rs"
    aliases = source/"src/exports.rs"
    root.write_text(ROOT_SOURCE)
    aliases.write_text(EXPORT)
    selection = dict(format="oddlang-rust-public-record-selection-v1", libSha256=sha(root),
        modules=[
            dict(id=ROOT+":sampling", name="sampling", records={"Reading": ROOT+":reading"}),
            dict(id=ROOT+":exports", name="exports", aliases={"Archived": ROOT+":archived"}),
        ])
    tool_input = work/"selection.json"

    def recover(output, error=None, *, repin=True):
        if repin:
            for module,path in zip(selection["modules"],(records,aliases)):
                module["sha256"] = sha(path)
        write_json(tool_input, selection)
        run(CLI, "recover", "public-records", root, tool_input, output, cwd=work, error=error)
        assert output.exists() != bool(error)

    records.write_text(DECLARATION)
    baseline = work/"baseline"
    recover(baseline)
    baseline_graph = json.loads((baseline/"graph.json").read_bytes())
    shape = baseline_graph["modules"][0]["records"][0]
    assert [(f["wire"],f["type"]) for f in shape["fields"]] == [
        ("label","text"),("epoch","u64"),("flags","octets"),("active","bool")]
    assert baseline_graph["modules"][1]["aliases"] == [
        dict(id=ROOT+":archived",target=ROOT+":reading")]
    assert b"must not enter" not in (baseline/"graph.json").read_bytes()
    for id,path in (("sampling",records),("exports",aliases)):
        assert sha(path) in (baseline/"recovery.lock").read_text()
    assert not any(p.suffix == ".rs" for p in baseline.iterdir())

    deps = dict(serde=dict(version="1.0.229",features=["derive"]),
                serde_json=dict(version="1.0.151"))
    (source/"Cargo.toml").write_text("""
[package]
name = "indicator-core"
version = "0.1.0"
edition = "2021"
[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
""")
    (source/"examples").mkdir()
    (source/"examples/probe.rs").write_text(CONSUMER)
    run("cargo","generate-lockfile","--offline",cwd=source)
    cargo = work/"Cargo.lock"
    shutil.copyfile(source/"Cargo.lock",cargo)
    def observed(crate):
        return run("cargo","run","--example","probe","--locked","--offline","--quiet",cwd=crate)

    baseline_out = b'{"label":"East\\nDrain","epoch":18446744073709551615,"flags":[0,255],"active":false}\n'
    mutant_out = b'{"epoch":18446744073709551615,"label":"East\\nDrain","flags":[0,255],"active":false}\n'
    assert observed(source) == baseline_out
    swapped = DECLARATION.replace("    pub label: String,\n    pub epoch: u64,",
                                 "    pub epoch: u64,\n    pub label: String,")
    records.write_text(swapped)
    recover(work/"stale","source digest",repin=False)
    mutated = work/"mutated"
    recover(mutated)
    assert observed(source) == mutant_out
    assert (baseline/"graph.json").read_bytes() != (mutated/"graph.json").read_bytes()

    records.write_text(DECLARATION.replace("#[derive", "#[cfg(feature = \"optional\")]\n#[derive"))
    recover(work/"conditional","unsupported record attribute")
    records.write_text(DECLARATION.replace("Vec<u8>", "Vec<u16>"))
    recover(work/"unknown-type","unsupported public record field type")
    records.write_text("use crate::CustomString as String;\n" + DECLARATION)
    recover(work/"shadowed-type","shadowed public record type")
    records.write_text(DECLARATION)
    selection["modules"][1]["aliases"]["Archived"] = ROOT+":reading"
    recover(work/"duplicate-id","duplicate public record identity")
    selection["modules"][1]["aliases"]["Archived"] = ROOT+":archived"
    aliases.write_text("pub use crate::missing::{Reading as Archived};")
    recover(work/"unresolved-alias","unresolved public record re-export")
    aliases.write_text('#![cfg(unix)]\n'+EXPORT)
    recover(work/"inner-cfg","unsupported inner public record module attribute")
    aliases.write_text(EXPORT)
    records.write_text("macro_rules! shadow { () => { pub struct String; } }\nshadow!();\n"+DECLARATION)
    recover(work/"macro-shadow","unsupported expanded item")
    inject = '#[path="scope.rs"] mod scope;\nuse self::scope::*;\n'
    scope = records.parent/"scope.rs"
    scope.write_text("pub(crate) struct Extra;\n")
    records.write_text(inject+DECLARATION)
    recover(work/"unresolved-glob","unpinned public record glob source")
    selection["modules"][0]["scopeGlobs"] = {"scope":sha(scope)}
    recover(work/"pinned-glob")
    assert (work/"pinned-glob/graph.json").read_bytes() == (baseline/"graph.json").read_bytes()
    scope.write_text("pub(crate) struct String;\n")
    selection["modules"][0]["scopeGlobs"]["scope"] = sha(scope)
    recover(work/"glob-shadow","shadowed public record type through glob")
    shutil.rmtree(source)

    manifest = dict(format="oddlang-native-package-v1", name="indicator-core",
        edition="2021", version="0.1.0", dependencies=deps, cargoLock=pin(work,cargo))
    for name, recovered, expected in (("baseline",baseline,baseline_out),
                                       ("mutated",mutated,mutant_out)):
        manifest["components"] = [dict(kind="public-records",module="record_views",
            graph=pin(work,recovered/"graph.json"),overlay=pin(work,recovered/"rust.json"),
            reexports=["sampling","exports"])]
        project = work/("project-"+name)
        run(CLI,"build","native-package","rust",write_json(work/"package.json",manifest),
            project,cwd=work)
        (project/"examples").mkdir()
        (project/"examples/probe.rs").write_text(CONSUMER)
        assert observed(project) == expected
        lock = json.loads((project/"package.lock").read_bytes())
        assert lock["components"][0]["projection"]["graphSha256"] == sha(recovered/"graph.json")
        assert b"must not enter" not in (project/"src/group_0/__projection_0.rs").read_bytes()
    print("Source recovery, graph-only projection and exact public record alias identity match baseline and mutation.")
