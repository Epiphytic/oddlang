#!/usr/bin/env python3
"""Recover a deliberately differently named and ordered build record."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

cli = Path(sys.argv[1]).resolve()


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def pin(base, path):
    return {"path": path, "sha256": hashlib.sha256((base / path).read_bytes()).hexdigest()}


with tempfile.TemporaryDirectory() as temp:
    base = Path(temp)
    write(base / "rover/lib.rs", "mod compile; mod format;\n")
    write(base / "rover/compile.rs", '''
use serde::Serialize;
#[derive(Serialize)]
pub(crate) struct Identification {
    #[serde(rename = "storage-ids")]
    pub stages: [u16; 3],
    pub origin: &'static str,
    pub envelope: u8,
    pub platform: &'static str,
    pub software: &'static str,
}
pub(crate) fn identification() -> Identification {
    Identification {
        envelope: 9,
        software: env!("CARGO_PKG_VERSION"),
        origin: env!("ROVER_SOURCE"),
        stages: [library::PUBLIC as u16, u16::from(crate::format::EARLIEST), crate::format::LATEST],
        platform: env!("ROVER_MACHINE"),
    }
}
''')
    write(base / "rover/format.rs", "pub(crate) const EARLIEST: u8 = 5; pub(crate) const LATEST: u16 = 100;")
    write(base / "library/lib.rs", "mod stored; pub const PUBLIC: i64 = stored::NEGATIVE;")
    defs = base / "library/stored.rs"
    write(defs, "pub(crate) const NEGATIVE: i64 = -3;")
    paths = ("rover/lib.rs", "rover/compile.rs", "rover/format.rs", "library/lib.rs", "library/stored.rs")
    selection = base / "selection.json"

    def recover(dest):
        write(selection, json.dumps({
            "format": "oddlang-build-report-recovery-v1", "id": "urn:rover:about",
            "crate": "rover", "module": ["compile"], "record": "Identification",
            "constructor": "identification", "targetEnv": "ROVER_MACHINE",
            "revisionEnv": "ROVER_SOURCE", "revision": "2" * 40,
            "roots": {"rover": "rover/lib.rs", "library": "library/lib.rs"},
            "sources": [pin(base, path) for path in paths],
        }))
        result = subprocess.run([cli, "recover", "build-report", base, selection, dest],
                                capture_output=True, text=True)
        return result

    out = base / "recovered"
    result = recover(out)
    assert result.returncode == 0, result.stderr
    graph = json.loads((out / "graph.json").read_bytes())
    assert [field["wire"] for field in graph["fields"]] == [
        "storage-ids", "origin", "envelope", "platform", "software"
    ]
    first = graph["fields"][0]["source"]
    assert first["kind"] == "unsigned-reference-list" and first["bits"] == 16
    assert [entry["conversion"] for entry in first["items"]] == ["wrap", "widen", "identity"]
    assert [d["value"] for d in first["items"][0]["chain"] if "value" in d] == [-3]
    receipt = json.loads((out / "observation.json").read_bytes())
    assert receipt["values"] == [[65533, 5, 100], "2" * 40, 9, None, None]
    repeat = base / "repeat"
    assert recover(repeat).returncode == 0
    for filename in ("graph.json", "observation.json"):
        assert (out / filename).read_bytes() == (repeat / filename).read_bytes()

    write(defs, "pub(crate) const NEGATIVE: i64 = 70000;")
    mutated = base / "mutated"
    assert recover(mutated).returncode == 0
    assert json.loads((mutated / "observation.json").read_bytes())["values"][0] == [4464, 5, 100]
    write(defs, "#[cfg(any())] pub(crate) const NEGATIVE: i64 = 70000;")
    rejected = recover(base / "rejected")
    assert rejected.returncode != 0 and "conditional" in rejected.stderr
    assert not (base / "rejected").exists()
    write(defs, "const NEGATIVE: i64 = 70000;")
    rejected = recover(base / "private")
    assert rejected.returncode != 0 and "inaccessible" in rejected.stderr
    write(defs, "pub(crate) const NEGATIVE: é = 70000;")
    rejected = recover(base / "type")
    assert rejected.returncode != 0 and "unsupported integer type" in rejected.stderr
    write(defs, "pub(crate) const NEGATIVE: i64 = 7;")
    declaration = base / "rover/compile.rs"
    declaration.write_text("use crate::format as library;\n" + declaration.read_text())
    rejected = recover(base / "shadow")
    assert rejected.returncode != 0 and "shadow external crate" in rejected.stderr
