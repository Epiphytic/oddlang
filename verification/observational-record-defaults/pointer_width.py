"""Check native word width and requalification after a source mutation."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1")
SOURCE = """
#[derive(Debug, Default)]
pub struct Contour {
    pub label: String,
    pub cells: usize,
}
"""


def run(*argv, work, failure=None):
    proc = subprocess.run(argv, cwd=work, env=ENV, capture_output=True, text=True)
    if failure:
        assert proc.returncode and failure in proc.stderr, proc
    else:
        assert not proc.returncode, proc
    return proc.stdout


def recover(work, source, failure=None):
    work.mkdir()
    (work / "source.rs").write_text(source)
    selection = {
        "format": "oddlang-rust-root-record-selection-v1",
        "sourceSha256": hashlib.sha256(source.encode()).hexdigest(),
        "id": "urn:terrain:root",
        "records": {"Contour": "urn:terrain:contour"},
    }
    (work / "selection.json").write_text(json.dumps(selection))
    run(CLI, "recover", "public-root-records", "source.rs", "selection.json", "model",
        work=work, failure=failure)
    if failure:
        assert not (work / "model").exists()
        return
    run(CLI, "build", "public-records", "rust", "model/graph.json", "model/rust.json", "projection",
        work=work)
    return json.loads((work / "model/graph.json").read_bytes())


with tempfile.TemporaryDirectory(prefix="odd-pointer-record-") as tmp:
    root = Path(tmp)
    for kind, native in (("pointer-unsigned", "usize"), ("u32", "u32")):
        work = root / kind
        source = SOURCE.replace("usize", native)
        graph = recover(work, source)
        field = graph["modules"][0]["records"][0]["fields"][1]
        assert field["type"] == kind and field["wire"] == "cells", graph
        probe = f"""
fn main() {{
    let a = Contour::default();
    let zero: ::core::primitive::{native} = a.cells;
    assert_eq!(zero, 0);
    let max = Contour {{ label: "ridge".into(), cells: ::core::primitive::{native}::MAX }};
    println!("{{}}:{{}}:{{}}", max.label, max.cells, ::core::primitive::{native}::BITS);
}}
"""
        results = []
        for label, implementation in (("original", source), ("generated", 'include!("projection/records.rs");')):
            (work / "probe.rs").write_text(implementation + probe)
            run("rustc", "--edition=2021", "probe.rs", "-o", label, work=work)
            results.append(run(work / label, work=work))
        assert results[0] == results[1]
    recover(root / "shadow", "type r#usize = u32;\n" + SOURCE, "shadowed public record")
    recover(root / "cfg", SOURCE.replace("pub cells", '#[cfg(target_pointer_width = "64")] pub cells'),
            "unsupported non-public or attributed field")

print("Pointer-width fields, fixed-width source mutation and explicit shape rejections pass.")

with tempfile.TemporaryDirectory(prefix="odd-pointer-module-") as tmp:
    root = Path(tmp)
    (root / "src").mkdir()
    lib = root / "src/lib.rs"
    lib.write_text("pub mod contour;\n")
    manifest = root / "Cargo.toml"
    manifest.write_text('[package]\nname="terrain"\nversion="0.0.0"\nedition="2024"\n')
    selection = {
        "format": "oddlang-rust-public-record-selection-v1",
        "libSha256": hashlib.sha256(lib.read_bytes()).hexdigest(),
        "cargoSha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
        "modules": [{"id": "urn:terrain:module", "name": "contour",
                     "records": {"Contour": "urn:terrain:contour"}}],
    }

    def module_recovery(source, output, failure=None):
        part = root / "src/contour.rs"
        part.write_text(source)
        selection["modules"][0]["sha256"] = hashlib.sha256(part.read_bytes()).hexdigest()
        (root / "selection.json").write_text(json.dumps(selection))
        run(CLI, "recover", "public-records", lib, "selection.json", output, work=root, failure=failure)
        assert (root / output).exists() != bool(failure)

    extern_glob = "use catalog::shapes::*;\n"
    module_recovery(extern_glob + SOURCE, "strict", "unsupported public record glob source")
    selection["format"] = "oddlang-rust-public-record-candidate-selection-v1"
    module_recovery(extern_glob + SOURCE, "candidate")
    lock = json.loads((root / "candidate/recovery.lock").read_bytes())
    assert lock["unresolvedGlobBindings"] == {"contour": ["catalog::shapes::*"]}
    assert "independent native compilation" in lock["gap"]
    module_recovery(extern_glob + SOURCE, "repeat")
    assert (root / "candidate/recovery.lock").read_bytes() == (root / "repeat/recovery.lock").read_bytes()
    module_recovery("use crate::catalog::*;\n" + SOURCE, "local-shadow", "unsupported public record glob source")
    module_recovery("#[cfg(unix)] " + extern_glob + SOURCE, "conditional-glob",
                    "unsupported public record glob source")

print("Module candidates record unresolved qualified glob bindings and preserve strict controls.")
