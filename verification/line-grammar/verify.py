"""Run an unrelated typed native inventory record through the line grammar."""
import copy
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

    for tag, change, message in [
        ("format", lambda g, r: g.update(format="else"), "invalid bounded"),
        ("zero", lambda g, r: g.update(maxBytes=0), "invalid bounded"),
        ("rows", lambda g, r: g.update(rows=[]), "invalid bounded"),
        ("empty row", lambda g, r: g["rows"].append([]), "invalid bounded"),
        ("LF", lambda g, r: g["forbiddenBytes"].append(10), "invalid bounded"),
        ("non-ASCII", lambda g, r: g["forbiddenBytes"].append(255), "invalid bounded"),
        ("duplicate byte", lambda g, r: g["forbiddenBytes"].append(9), "invalid bounded"),
        ("space", lambda g, r: g["forbiddenBytes"].append(32), "invalid bounded"),
        ("forbidden literal", lambda g, r: g["forbiddenBytes"].append(ord("k")), "literal token"),
        ("forbidden grammar text", lambda g, r: g["forbiddenBytes"].append(ord("v")), "admissible token"),
        ("unused", lambda g, r: g["rows"][1][1].pop("grammar"), "unused nested"),
        ("unresolved", lambda g, r: g["rows"][1][1].update(grammar="urn:missing"), "unresolved nested"),
        ("duplicate grammar", lambda g, r: g["textGrammars"].extend(g["textGrammars"]), "duplicate nested"),
        ("decimal bounds", lambda g, r: g["rows"][3][-1].update(bits=8, maximum=256), "decimal bounds"),
        ("odd hex", lambda g, r: g["rows"][3][3].update(digits=3), "hexadecimal extent"),
        ("boolean", lambda g, r: g["rows"][4][1].update(trueToken="no"), "boolean encoding"),
        ("integer width", lambda g, r: g["rows"][3][4].update(bits=8), "integer-valued"),
        ("capture duplicate", lambda g, r: g["rows"][3].append(decimal(32, 1, capture="amount")), "duplicate capture"),
        ("overlay missing", lambda g, r: r["captureNames"].pop("batch"), "distinct valid names"),
        ("overlay extra", lambda g, r: r["captureNames"].update(extra="extra"), "distinct valid names"),
        ("overlay invalid", lambda g, r: r["captureNames"].update(batch="new-batch"), "distinct valid names"),
        ("overlay duplicate", lambda g, r: r["captureNames"].update(batch="item"), "distinct valid names"),
        ("overlay collision", lambda g, r: r.update(reader="Report"), "distinct valid names"),
    ]:
        wrong_g, wrong_r = copy.deepcopy(graph), copy.deepcopy(overlay)
        change(wrong_g, wrong_r)
        g.write_text(json.dumps(wrong_g))
        r.write_text(json.dumps(wrong_r))
        rejected = stage / "rejected"
        output = run(CLI, "build", "line-grammar", "rust", g, r, rejected, success=False)
        assert message in output and not rejected.exists(), (tag, output)

    g.write_text(json.dumps({
        "format": graph["format"], "id": "urn:inventory:largest-count", "maxBytes": 32,
        "forbiddenBytes": [9, 13],
        "rows": [[decimal(64, 0, capture="large")]],
    }))
    r.write_text(json.dumps({"format": overlay["format"], "reader": "largest", "record": "Maximum",
                             "captureNames": {"large": "large"}}))
    run(CLI, "build", "line-grammar", "rust", g, r, stage / "static-record")
    g.write_text(json.dumps({
        "format": graph["format"], "id": "urn:inventory:hex-and-text", "maxBytes": 32,
        "forbiddenBytes": [9, 13],
        "rows": [[{"kind": "radix16", "digits": 2, "decode": False, "capture": "hex"},
                  {"kind": "text", "capture": "marker"}]],
    }))
    r.write_text(json.dumps({"format": overlay["format"], "reader": "lookup", "record": "Option",
                             "captureNames": {"hex": "hex", "marker": "marker"}}))
    run(CLI, "build", "line-grammar", "rust", g, r, stage / "text-record")
    run("rustc", "+1.97.1", "--edition=2021", "-Dwarnings", HERE / "extras.rs", "-o", program,
        env=dict(os.environ, STATIC=str(stage / "static-record/lines.rs"),
                 TEXT=str(stage / "text-record/lines.rs")))
    run(program)
print("Ordered line grammar native typed and malformed cases passed")
