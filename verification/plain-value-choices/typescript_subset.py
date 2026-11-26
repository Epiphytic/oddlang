"""Require the copyable TypeScript profile to reject unsupported choice capabilities."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

COMPILER = Path(sys.argv[1]).resolve()
TYPE = "urn:choice-fixture:slope:type"
CASE = TYPE + ":Below"


def write(file, data):
    file.write_text(json.dumps(data, indent=2) + "\n")


with tempfile.TemporaryDirectory(prefix="checked-choice-ts-subset-") as directory:
    root = Path(directory)
    graph, overlay = root / "graph.json", root / "typescript.json"
    write(overlay, {"format": "oddlang-checked-values-typescript-v1",
                    "types": {TYPE: "Slope"}, "variants": {CASE: "Below"}})

    def build(label, attributes, compiler=COMPILER):
        write(graph, {"format": "oddlang-checked-values-v1", "id": "urn:choice-fixture:slope",
                      "types": {TYPE: {"kind": "choice", "cases": [CASE], **attributes}}})
        target = root / label
        result = subprocess.run([compiler, "build", "checked-values", "typescript", graph, overlay, target],
                                capture_output=True, timeout=45)
        return result, target

    supported, output = build("plain", {})
    assert supported.returncode == 0 and (output / "values.ts").is_file(), supported.stderr
    if len(sys.argv) > 2:
        previous, old_output = build("previous", {}, Path(sys.argv[2]).resolve())
        assert previous.returncode == 0, previous.stderr
        for name in ("values.ts", "obligations.json", "projection.lock"):
            data = (output / name).read_bytes()
            assert data == (old_output / name).read_bytes(), name
            print(name, hashlib.sha256(data).hexdigest())
    for label, graph_fields in (("clone-only", {"copy": False}), ("derived-debug", {"debug": True})):
        failure, output = build(label, graph_fields)
        assert failure.returncode and not output.exists(), (label, failure.stderr)
        assert f"TypeScript checked choices do not support {TYPE}".encode() in failure.stderr
print("TypeScript projects the plain choice and rejects clone-only and derived Debug choices.")
