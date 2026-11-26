"""Resolve independent callers across two revisions of a small unrelated library."""

import argparse
from pathlib import Path
import os
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
        "old": """pub struct Held; pub struct Gone; pub struct Hidden; pub struct Meter; pub struct Mac; pub struct Clash; mod marker {pub trait Tag {}} pub use marker::Tag as _;
                pub mod removed { pub struct Leaf; }""",
        "new": """pub struct Held; mod Hidden {} pub const Meter: u8 = 3;
                #[macro_export] macro_rules! Mac { () => { 2 }; }
                mod a { pub struct Clash; } mod b { pub struct Clash; } pub use a::*; pub use b::*;""",
    }.items():
        pkg = root / name
        (pkg / "src").mkdir(parents=True)
        manifest = pkg / "Cargo.toml"
        manifest.write_text('[package]\nname="fixture"\nversion="1.0.0"\nedition="2024"\n')
        (pkg / ".cargo").mkdir()
        (pkg / ".cargo/config.toml").write_text(
            '[build]\nrustflags=["--cfg","local_profile"]\nrustdocflags=["--cfg","local_profile"]\n'
        )
        (pkg / "src/lib.rs").write_text(text + "\n#[cfg(local_profile)] pub struct LocalType;")
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
    assert result["expectedUnresolved"] == [["_"]]
    rows = {tuple(r["spelling"]): r for r in result["imports"]}
    assert rows["LocalType",]["importCompiles"] and rows["LocalType",]["subjectKind"] == "data"
    assert rows["Held",]["importCompiles"] and rows["Held",]["subjectKind"] == "data"
    assert rows["Gone",]["errorCodes"] == ["E0432"] and not rows["Gone",]["importCompiles"]
    assert rows["Hidden",]["errorCodes"] == ["E0603"] and not rows["Hidden",]["importCompiles"]
    assert rows["Meter",]["importCompiles"] and rows["Meter",]["subjectKind"] is None
    assert rows["Mac",]["importCompiles"] and rows["Mac",]["subjectKind"] is None
    assert not rows["Clash",]["importCompiles"] and rows["Clash",]["subjectAmbiguity"]
    assert rows["Clash",]["errorCodes"] == ["ambiguous_glob_imports"]
    assert not rows["removed", "Leaf"]["importCompiles"]
    inverse = probe(
        root / "new-observed/recovered/exports.json",
        root / "old-observed/recovered",
        root / "old/Cargo.toml",
        root / "old-observed/native",
    )
    conflict = next(row for row in inverse["imports"] if row["spelling"] == ["Clash"])
    assert (
        conflict["expectedAmbiguity"]
        and conflict["importCompiles"]
        and conflict["subjectKind"] == "data"
    )
    # Pin driver-side Cargo configuration even with an unchanged package and lockfile.
    original_cwd = Path.cwd()
    (root / ".cargo").mkdir()
    (root / ".cargo/config.toml").write_text('[build]\nrustflags=["--cfg","unreviewed"]\n')
    os.chdir(root)
    try:
        try:
            probe(
                root / "old-observed/recovered/exports.json",
                root / "new-observed/recovered",
                root / "new/Cargo.toml",
                root / "new-observed/native",
            )
        except ValueError as error:
            assert "configuration differs" in str(error)
        else:
            raise AssertionError("accepted unpinned Cargo flags")
    finally:
        os.chdir(original_cwd)
    print(
        "Pinned downstream imports distinguish missing, private and ambiguous paths and capture successful imports outside the type table."
    )
