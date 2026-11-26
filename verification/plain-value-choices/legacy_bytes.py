"""Check byte preservation against a compiler built before capability extensions."""

import hashlib
import json
import subprocess
import tempfile
from pathlib import Path
import sys

older, newer = (Path(binary).resolve() for binary in sys.argv[1:])


def run(*args):
    subprocess.run(args, capture_output=True, text=True, check=True)


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    source = root / "compass.rs"
    selection = root / "selection.json"
    text = "#[derive(PartialEq, Copy, Eq, Clone)]\npub enum Compass { Inland, Coast, Ridge }\n"
    source.write_text(text)
    selection.write_text(
        json.dumps(
            dict(
                format="oddlang-plain-choice-selection-v1",
                id="urn:compass:v1",
                name="Compass",
                sourceSha256=hashlib.sha256(text.encode()).hexdigest(),
            )
        )
    )
    for label, cli in [("baseline", older), ("candidate", newer)]:
        result = root / label
        run(cli, "recover", "plain-choice", source, selection, result)
        run(
            cli,
            "build",
            "checked-values",
            "rust",
            result / "graph.json",
            result / "rust.json",
            root / (label + "-rust"),
        )
    for suffix, names in (
        ("", ("graph.json", "rust.json")),
        ("-rust", ("values.rs", "obligations.json")),
    ):
        for name in names:
            old = (root / ("baseline" + suffix) / name).read_bytes()
            assert old == (root / ("candidate" + suffix) / name).read_bytes(), name
            print(name, hashlib.sha256(old).hexdigest())
