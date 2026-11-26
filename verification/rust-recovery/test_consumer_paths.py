"""Resolve independent callers across two revisions of a small unrelated library."""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from probe_consumer_paths import probe
from probe_exports import pinned_environment

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("oddlang", type=Path)
args = parser.parse_args()
here = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    for name, text in {
        "old": "pub struct Held; pub struct Gone; pub struct Hidden; pub struct Meter;",
        "new": "pub struct Held; mod Hidden {} pub const Meter: u8 = 3;",
    }.items():
        pkg = root / name
        (pkg / "src").mkdir(parents=True)
        manifest = pkg / "Cargo.toml"
        manifest.write_text('[package]\nname="fixture"\nversion="1.0.0"\nedition="2024"\n')
        (pkg / "src/lib.rs").write_text(text)
        subprocess.run(
            ["cargo", "+1.97.1", "generate-lockfile", "--offline", "--manifest-path", manifest],
            check=True,
        )
        subprocess.run(
            [
                sys.executable,
                "-B",
                here / "probe_exports.py",
                args.oddlang.resolve(),
                manifest,
                "fixture",
                root / (name + "-observed"),
                "--default-features",
            ],
            env=pinned_environment(),
            check=True,
        )
    result = probe(
        root / "old-observed/recovered/exports.json",
        root / "new-observed/recovered",
        root / "new/Cargo.toml",
        root / "new-observed/native",
    )
    rows = {tuple(r["spelling"]): r for r in result["imports"]}
    assert rows["Held",]["importCompiles"] and rows["Held",]["subjectKind"] == "data"
    assert rows["Gone",]["errorCodes"] == ["E0432"] and not rows["Gone",]["importCompiles"]
    assert rows["Hidden",]["errorCodes"] == ["E0603"] and not rows["Hidden",]["importCompiles"]
    assert rows["Meter",]["importCompiles"] and rows["Meter",]["subjectKind"] is None
    assert set(rows) == {("Held",), ("Gone",), ("Hidden",), ("Meter",)}
    print("Distinguished available types, private and missing imports, and a value-only replacement.")
