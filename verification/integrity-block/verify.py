#!/usr/bin/env python3
"""Qualify an unrelated locked consumer and reject malformed semantic graphs."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parent / "fixture"


def invoke(*args, failure=None):
    result = subprocess.run((CLI, *args), capture_output=True, text=True)
    if failure:
        assert result.returncode and failure in result.stderr, result.stderr
    else:
        assert result.returncode == 0, result.stderr


def write(path, document):
    path.write_text(json.dumps(document))
    return path


with tempfile.TemporaryDirectory(prefix="integrity-block-") as directory:
    root = Path(directory)
    consumer = root / "consumer"
    shutil.copytree(FIXTURE, consumer)
    graph, overlay = FIXTURE / "graph.json", FIXTURE / "rust.json"
    generated = root / "baseline"
    invoke("build", "integrity-block", "rust", graph, overlay, generated)
    shutil.copyfile(generated / "codec.rs", consumer / "src/codec.rs")
    repeat = root / "repeat"
    equivalent = json.loads(graph.read_text())
    reordered_object = dict(reversed(tuple(equivalent.items())))
    invoke("build", "integrity-block", "rust", write(root / "equivalent.json", reordered_object),
           overlay, repeat)
    for name in ("codec.rs", "obligations.json", "projection.lock"):
        assert (generated / name).read_bytes() == (repeat / name).read_bytes()
    lock = json.loads((generated / "projection.lock").read_bytes())
    assert lock["artifactSha256"] == hashlib.sha256((generated / "codec.rs").read_bytes()).hexdigest()

    obligations = (generated / "obligations.json").read_bytes()
    assert lock["obligationsSha256"] == hashlib.sha256(obligations).hexdigest()
    rows = json.loads(obligations)["obligations"]
    assert len(rows) == len({row["declaration"] for row in rows})
    assert all(row["qualification"] == "independent-witness-required" for row in rows)

    hintless = json.loads(graph.read_text())
    hintless.pop("providerHints")
    without = root / "without-hints"
    invoke("build", "integrity-block", "rust", write(root / "hintless.json", hintless), overlay, without)
    for name in ("codec.rs", "obligations.json"):
        assert (generated / name).read_bytes() == (without / name).read_bytes()

    identity = json.loads(graph.read_text())
    identity["representations"]["compressed"] = None
    identity_rust = json.loads(overlay.read_text())
    identity_rust["variants"].pop("compressed")
    identity_rust.pop("payloadHelper")
    identity_output = root / "identity"
    invoke("build", "integrity-block", "rust", write(root / "identity.json", identity),
           write(root / "identity-rust.json", identity_rust), identity_output)
    identity_source = (identity_output / "codec.rs").read_bytes()
    assert b"::flate2" not in identity_source
    (consumer / "src/stored.rs").write_bytes(identity_source)

    all_messages = json.loads(graph.read_text())
    all_messages["errorMessages"] = {
        identity: "wire " + role for role, identity in all_messages["errors"].items()
    }
    full = root / "all-messages"
    invoke("build", "integrity-block", "rust", write(root / "all-messages.json", all_messages), overlay, full)
    (consumer / "src/all_messages.rs").write_bytes((full / "codec.rs").read_bytes())
    invalid_binding = json.loads(overlay.read_text())
    invalid_binding["blockType"] = "u64"
    invoke("build", "integrity-block", "rust", graph,
           write(root / "primitive-name.json", invalid_binding), root / "bad-rust",
           failure="must not shadow a primitive")

    changed = json.loads(graph.read_text())
    changed["fields"][0]["wireName"] = "location"
    changed["bounds"]["offsetMaximum"] = 31
    changed["validateOrder"] = ["offset", "raw-size", "encoded-size", "representation-size"]
    output = root / "mutant"
    invoke("build", "integrity-block", "rust", write(root / "mutant.json", changed), overlay, output)
    shutil.copyfile(output / "codec.rs", consumer / "src/mutant.rs")

    narrower = json.loads(graph.read_text())
    narrower["bounds"]["encodedMaximum"] = 64
    bounded = root / "narrower"
    invoke("build", "integrity-block", "rust", write(root / "narrower.json", narrower), overlay, bounded)
    (consumer / "src/narrower.rs").write_bytes((bounded / "codec.rs").read_bytes())

    malformed = json.loads(graph.read_text())
    malformed["fields"][1]["wireName"] = "position"
    invoke("build", "integrity-block", "rust", write(root / "duplicate.json", malformed),
           overlay, root / "duplicate", failure="distinct wire names")
    malformed = json.loads(graph.read_text())
    malformed["decodeOrder"].reverse()
    invoke("build", "integrity-block", "rust", write(root / "order.json", malformed),
           overlay, root / "order", failure="decode precedence")
    malformed = json.loads(graph.read_text())
    malformed["representations"]["compressed"]["dictionary"] = "allowed"
    invoke("build", "integrity-block", "rust", write(root / "dictionary.json", malformed),
           overlay, root / "dictionary", failure="unsupported compression policy")
    assert all(not (root / name).exists() for name in ("duplicate", "order", "dictionary"))
    subprocess.run(["cargo", "+1.97.1", "run", "--locked", "--offline", "--quiet"],
                   cwd=consumer, check=True)
print("integrity-block qualification passed")
