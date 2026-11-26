"""Run an unrelated typed native inventory record through the line grammar."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent


def run(*argv, env=None, success=True):
    result = subprocess.run(argv, capture_output=True, text=True, env=env, timeout=120)
    if (result.returncode == 0) != success:
        raise ValueError(f"{argv}\n{result.stdout}{result.stderr}")
    return result.stderr


def atom(text, **other):
    return {"kind": "literal", "text": text, **other}


def decimal(bits, minimum, **other):
    return {"kind": "decimal", "bits": bits, "minimum": minimum, **other}


with tempfile.TemporaryDirectory(prefix="odd-line-fixture-") as tmp:
    stage = Path(tmp)
    ref = "urn:inventory:batch-grammar"
    graph = {
        "format": "oddlang-line-grammar-v1",
        "id": "urn:inventory:receipt",
        "maxBytes": 120,
        "forbiddenBytes": [9, 13],
        "textGrammars": [{"format": "oddlang-text-grammar-v1", "id": ref, "prefixes": ["v5."],
                          "body": {"notation": "canonical-ascii-decimal", "bits": 16, "minimum": 0},
                          "suffix": {"delimiter": "-rc.", "body": {
                              "notation": "canonical-ascii-decimal", "bits": 16, "minimum": 1}},
                          "consumption": "entire-input"}],
        "rows": [
            [atom("stock"), atom("1")],
            [atom("batch"), {"kind": "text", "capture": "batch", "grammar": ref}],
            [atom("track"), {"kind": "split", "capture": "track", "delimiter": "=",
                "left": {"kind": "text", "capture": "family", "choices": ["east", "west"]},
                "right": {"kind": "split", "delimiter": ".", "left": decimal(16, 0),
                          "right": decimal(8, 1, capture="count")}}],
            [atom("unit"), atom("probe", capture="item"),
                {"kind": "split", "delimiter": "=", "left": {
                    "kind": "text", "choices": ["A", "B"]}, "right": decimal(8, 1)},
                {"kind": "radix16", "capture": "hash", "digits": 4, "decode": True},
                {"kind": "integer-token", "text": "0664", "bits": 16, "value": 436, "capture": "limit"}],
            [atom("ready"), {"kind": "boolean", "capture": "ready", "falseToken": "no", "trueToken": "yes"}],
            [atom("end")],
        ],
    }
    # A separate numeric value shares a row with a fixed lexical/numeric pair.
    graph["rows"][3].append(decimal(32, 1, maximum=16, capture="amount"))
    overlay = {
        "format": "oddlang-line-captures-rust-v1",
        "reader": "read_report",
        "record": "Report",
        "captureNames": {name: name for name in
                         ("batch", "track", "family", "count", "item", "hash", "limit", "amount", "ready")},
    }
    g, r = stage / "grammar.json", stage / "rust.json"
    g.write_text(json.dumps(graph))
    r.write_text(json.dumps(overlay))
    for tag in ("first", "second"):
        run(CLI, "build", "line-grammar", "rust", g, r, stage / tag)
    out = stage / "first"
    for name in ("lines.rs", "projection.lock"):
        assert (out / name).read_bytes() == (stage / "second" / name).read_bytes()
    pin = json.loads((out / "projection.lock").read_bytes())
    assert pin["sourceSha256"] == hashlib.sha256((out / "lines.rs").read_bytes()).hexdigest()
    program = stage / "caller"
    run("rustc", "+1.97.1", "--edition=2021", HERE / "inventory.rs", "-o", program,
        env=dict(os.environ, GENERATED=str(out / "lines.rs")))
    run(program)

    graph["rows"][3].append(decimal(32, 1, capture="amount"))
    g.write_text(json.dumps(graph))
    failure = stage / "rejected"
    assert "duplicate capture" in run(CLI, "build", "line-grammar", "rust", g, r, failure, success=False)
    assert not failure.exists()
print("Ordered line grammar native typed and malformed cases passed")
