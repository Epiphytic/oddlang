"""Compile crate-scoped native views of a text subgrammar and ordered choices."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent


def run(*argv, env=None, reject=False):
    completed = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=120)
    if (completed.returncode != 0) != reject:
        raise ValueError(f"{argv}\n{completed.stdout}{completed.stderr}")
    return completed.stderr


grammar = "urn:instrument:branch"
failure = "urn:instrument:failure"
graph = {
    "format": "oddlang-line-grammar-v1",
    "id": "urn:instrument:configuration",
    "maxBytes": 64,
    "forbiddenBytes": [9, 13],
    "textGrammars": [{
        "format": "oddlang-text-grammar-v1", "id": grammar, "prefixes": ["b"],
        "body": {"notation": "canonical-ascii-decimal", "bits": 16, "minimum": 1},
        "consumption": "entire-input",
    }],
    "rows": [
        [{"kind": "literal", "text": "branch"}, {"kind": "text", "grammar": grammar}],
        [{"kind": "literal", "text": "tier"}, {"kind": "text", "capture": "tier", "choices": ["bronze", "copper"]}],
    ],
    "recordResult": {
        "failure": {"id": failure, "code": "invalid_configuration"},
        "records": {"configuration": [{"id": "tier", "type": {"kind": "owned-text"}}]},
        "entry": {"record": "configuration", "fields": {"tier": {"kind": "capture", "id": "tier"}}},
        "formatters": [],
    },
}
overlay = {
    "format": "oddlang-line-record-rust-v1", "parser": "parse", "visibility": "crate",
    "error": {"id": failure, "type": ["crate", "Fault"], "variant": "Invalid"},
    "records": {"configuration": {"name": "Config", "fields": {"tier": "tier"}, "derives": []}},
    "formatters": {},
    "support": {"nestedGrammars": {grammar: "is_branch"}, "textChoices": {"tier": "TIERS"}},
}
with tempfile.TemporaryDirectory(prefix="oddlang-line-support-") as scratch:
    tmp = Path(scratch)
    g, b, out = (tmp / n for n in ("graph.json", "overlay.json", "out"))
    g.write_text(json.dumps(graph))
    b.write_text(json.dumps(overlay))
    run(CLI, "build", "line-grammar", "rust", g, b, out)
    native = tmp / "consumer"
    run("rustc", "+1.97.1", "--edition=2021", "-Dwarnings", HERE / "support.rs", "-o", native,
        env=dict(os.environ, GENERATED=str(out / "lines.rs")))
    run(native)
    for choices, nested, expected in [
        ({"tier": "is_branch"}, {grammar: "is_branch"}, "distinct native support names"),
        ({"missing": "TIERS"}, {grammar: "is_branch"}, "unknown finite text choice capture"),
        ({"tier": "TIERS"}, {"urn:missing": "is_branch"}, "unknown nested text grammar"),
    ]:
        overlay["support"] = {"nestedGrammars": nested, "textChoices": choices}
        b.write_text(json.dumps(overlay))
        failed = tmp / "not-published"
        assert expected in run(CLI, "build", "line-grammar", "rust", g, b, failed, reject=True)
        assert not failed.exists()
print("Native crate-visible grammar and finite-choice declarations passed")
