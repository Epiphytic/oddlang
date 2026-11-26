#!/usr/bin/env python3
"""Compile an unrelated, locked Rust consumer of the endpoint-set projection."""

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

BINARY = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parent / "fixture"


def oddlang(*args, expected=None):
    result = subprocess.run([BINARY, *args], text=True, capture_output=True)
    if expected is None:
        assert result.returncode == 0, result.stderr
        return result.stdout.strip()
    assert result.returncode != 0 and expected in result.stderr, result.stderr


def write(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


with tempfile.TemporaryDirectory(prefix="endpoint-set-qualify-") as temp:
    root = Path(temp)
    graph = FIXTURE / "policy.json"
    overlay = FIXTURE / "rust.json"
    graph_pin = oddlang("endpoint-set", "digest", graph)
    overlay_pin = oddlang("endpoint-set", "overlay-digest", overlay)
    generated = root / "bound"
    oddlang("build", "endpoint-set", "rust", graph, overlay, generated,
            "--graph-sha256", graph_pin, "--overlay-sha256", overlay_pin)

    same_graph = json.loads(graph.read_bytes())
    same_graph["errors"].reverse()
    same_graph["rules"][-2]["ip"]["loopback"].reverse()
    same_graph["rules"][-3]["forbid"].reverse()
    sorted_graph = write(root / "reordered.json", same_graph)
    same_overlay = json.loads(overlay.read_bytes())
    same_overlay["derives"].reverse()
    sorted_overlay = write(root / "reordered-rust.json", same_overlay)
    assert oddlang("endpoint-set", "digest", sorted_graph) == graph_pin
    assert oddlang("endpoint-set", "overlay-digest", sorted_overlay) == overlay_pin
    split = root / "bound-repeat"
    oddlang("build", "endpoint-set", "rust", sorted_graph, sorted_overlay, split,
            "--graph-sha256", graph_pin, "--overlay-sha256", overlay_pin)
    for filename in ("endpoint_set.rs", "projection.lock"):
        assert (generated / filename).read_bytes() == (split / filename).read_bytes()

    free = root / "free"
    oddlang("build", "endpoint-set", "rust", graph, free, "--graph-sha256", graph_pin)

    changed_graph = json.loads(graph.read_bytes())
    changed_graph["empty"] = {"kind": "allow"}
    changed_graph["deduplicate"] = False
    del changed_graph["providerHint"]
    changed_graph["rules"][0], changed_graph["rules"][1] = changed_graph["rules"][1], changed_graph["rules"][0]
    changed_graph["rules"][-2]["dns"]["trailingDot"] = "preserve"
    changed = write(root / "different.json", changed_graph)
    changed_pin = oddlang("endpoint-set", "digest", changed)
    assert changed_pin != graph_pin
    different = root / "different"
    oddlang("build", "endpoint-set", "rust", changed, different, "--graph-sha256", changed_pin)

    oddlang("build", "endpoint-set", "rust", graph, root / "wrong",
            "--graph-sha256", "0" * 64, expected="graph digest mismatch")

    bad_graph = json.loads(graph.read_bytes())
    bad_graph["rules"][0], bad_graph["rules"][2] = bad_graph["rules"][2], bad_graph["rules"][0]
    oddlang("endpoint-set", "digest", write(root / "wrong-order.json", bad_graph),
            expected="input rule must precede parse")
    bad_graph = json.loads(graph.read_bytes())
    bad_graph["rules"][-2]["ip"]["ipv4"]["allow"] = ["198.51.100.42/24"]
    oddlang("endpoint-set", "digest", write(root / "host-bits.json", bad_graph),
            expected="host bits")
    bad_graph = json.loads(graph.read_bytes())
    bad_graph["rules"][0]["error"] = "urn:download-mirrors:unknown"
    oddlang("endpoint-set", "digest", write(root / "wrong-error.json", bad_graph),
            expected="undeclared error")

    bad_graph = json.loads(graph.read_bytes())
    bad_graph["rules"][3]["schemes"].append("git")
    oddlang("endpoint-set", "digest", write(root / "opaque-host.json", bad_graph),
            expected="host-policy requires a preceding allowed-schemes")

    bad_graph = json.loads(graph.read_bytes())
    bad_graph["rules"].pop()
    oddlang("endpoint-set", "digest", write(root / "missing-opt-in.json", bad_graph),
            expected="loopback classification requires loopback-opt-in")

    bad_graph = json.loads(graph.read_bytes())
    bad_graph["empty"] = {"kind": "allow", "error": "urn:download-mirrors:empty"}
    oddlang("endpoint-set", "digest", write(root / "extra-empty-field.json", bad_graph),
            expected="unknown field")

    bad_overlay = json.loads(overlay.read_bytes())
    del bad_overlay["errorVariants"]["urn:download-mirrors:empty"]
    incomplete = write(root / "incomplete.json", bad_overlay)
    incomplete_pin = oddlang("endpoint-set", "overlay-digest", incomplete)
    oddlang("build", "endpoint-set", "rust", graph, incomplete, root / "incomplete",
            "--graph-sha256", graph_pin, "--overlay-sha256", incomplete_pin,
            expected="error mapping must cover exactly")
    bad_overlay["typeName"] = "Bad; unsafe"
    oddlang("endpoint-set", "overlay-digest", write(root / "bad-ident.json", bad_overlay),
            expected="invalid Rust identifier")

    consumer = root / "consumer"
    shutil.copytree(FIXTURE, consumer)
    shutil.copy2(generated / "endpoint_set.rs", consumer / "src/projection.rs")
    shutil.copy2(free / "endpoint_set.rs", consumer / "src/free.rs")
    shutil.copy2(different / "endpoint_set.rs", consumer / "src/different.rs")
    manifest_path = consumer / "Cargo.toml"
    manifest = manifest_path.read_text(encoding="utf-8")
    assert manifest.count('edition = "2024"') == 1
    for edition in ("2021", "2024"):
        manifest_path.write_text(manifest.replace('edition = "2024"', f'edition = "{edition}"'),
                                 encoding="utf-8")
        subprocess.run(["cargo", "+1.97.1", "run", "--locked", "--offline", "--quiet",
                        "--target-dir", root / "consumer-target"], cwd=consumer, check=True)
print("endpoint-set qualification passed in Rust editions 2021 and 2024")
