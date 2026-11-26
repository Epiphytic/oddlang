#!/usr/bin/env python3
"""Recheck source observations and controlled changes on the pinned source."""

import argparse
from hashlib import sha256
import json
from pathlib import Path
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
SELECTION = HERE / "selection.json"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def change(source, old, new):
    require(source.count(old) == 1, f"expected a single mutation anchor: {old!r}")
    return source.replace(old, new, 1)


def check(executable, upstream):
    metadata = json.loads((HERE / "provenance.json").read_text())
    revision = subprocess.check_output(
        ["git", "-C", str(upstream), "rev-parse", "HEAD"], text=True
    ).strip()
    require(revision == metadata["revision"], "upstream revision does not match the pin")
    for entry in metadata["sources"].values():
        actual = sha256((upstream / entry["path"]).read_bytes()).hexdigest()
        require(actual == entry["sha256"], f"source digest changed: {entry['path']}")
    def graph_digest(graph):
        return subprocess.check_output(
            [str(executable), "tagged-wire", "digest", str(graph)], text=True
        ).strip()

    graph = HERE / metadata["contract"]
    digest = graph_digest(graph)
    require(digest == metadata["contractSha256"], "contract digest changed")
    overlay = HERE / metadata["overlay"]["file"]
    require(sha256(overlay.read_bytes()).hexdigest() == metadata["overlay"]["sha256"],
            "reviewed overlay digest changed")
    variant = metadata["overlay"]["variant"]
    selected = Path(json.loads(SELECTION.read_text())["file"])
    original = (upstream / selected).read_text()

    def extract(root, output=None):
        if output is None:
            args = ["tagged-variant", str(root), str(SELECTION)]
        else:
            args = [
                "tagged-contract", str(root), str(SELECTION), str(overlay), variant, str(output)
            ]
        return subprocess.run(
            [str(executable), "recover", *args],
            capture_output=True,
            text=True,
            check=False,
        )

    def materialize(root, output):
        result = extract(root, output)
        require(result.returncode == 0, result.stderr)
        return graph_digest(output)

    def successful(root):
        result = extract(root)
        require(result.returncode == 0, result.stderr)
        return json.loads(result.stdout)

    baseline = successful(upstream)
    require(baseline == json.loads((HERE / "candidate.json").read_text()), "baseline changed")
    expected_fields = [
        {
            "name": "offset",
            "position": 0,
            "kind": "uint-be",
            "width": 8,
            "writeLine": 396,
            "writerDefinitionLine": 751,
        },
        {"name": "bytes", "position": 1, "kind": "rest-bytes", "writeLine": 397},
    ]
    require(baseline["fields"] == expected_fields, "field order or shape changed")

    with tempfile.TemporaryDirectory(prefix="oddlang-tagged-") as temporary:
        root = Path(temporary)
        selected_copy = root / selected
        selected_copy.parent.mkdir(parents=True)
        require(materialize(upstream, root / "original.ttl") == digest,
                "overlay did not reproduce the pinned graph")
        changed_encoder = change(
            original,
            "payload.extend(bytes);\n                5\n",
            "payload.extend(bytes);\n                51\n",
        )
        selected_copy.write_text(changed_encoder)
        rejected = extract(root)
        require(
            rejected.returncode != 0 and "tags disagree" in rejected.stderr,
            "one-sided tag change was not diagnosed",
        )
        require(not rejected.stdout, "conflicting candidate was emitted")
        rejected_output = root / "rejected.ttl"
        rejected = extract(root, rejected_output)
        require(rejected.returncode != 0 and not rejected_output.exists(),
                "conflicting graph was emitted")

        changed_decoder = change(
            changed_encoder,
            "5 => {\n                let offset = r.offset()?;",
            "51 => {\n                let offset = r.offset()?;",
        )
        selected_copy.write_text(changed_decoder)
        changed = successful(root)
        def tags(report):
            return report["encodedTag"]["value"], report["decodedTag"]["value"]

        require(tags(baseline) == (5, 5) and tags(changed) == (51, 51), "tag mutation lost")
        require(changed["fields"] == baseline["fields"], "unrelated fields changed")
        expected_graph = root / "expected-retagged.ttl"
        expected_graph.write_text(change(graph.read_text(), "t:tag 5 ;", "t:tag 51 ;"))
        changed_digest = materialize(root, root / "retagged.ttl")
        require(changed_digest != digest and changed_digest == graph_digest(expected_graph),
                "materialized graph did not change exactly one tag")

        selected_copy.write_text(original.replace("put_u64", "put_be_integer"))
        renamed = successful(root)
        require(renamed["sourceSha256"] != baseline["sourceSha256"], "helper mutation absent")
        require(
            tags(renamed) == (5, 5) and renamed["fields"] == baseline["fields"],
            "equivalent helper rename changed observations",
        )

        require(materialize(root, root / "refactored.ttl") == digest,
                "equivalent helper rename changed the graph")

    print(f"Contract pin verified at {digest}.")
    print("Pinned tag 5 and two payload writes reproduced.")
    print(f"Tag 51 changed only the graph tag, yielding {changed_digest}.")
    print("A one-sided tag change was rejected without writing a graph.")
    print("Equivalent integer-writer rename preserved the observations and graph pin.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("oddlang", type=Path)
    parser.add_argument("upstream", type=Path)
    options = parser.parse_args()
    check(options.oddlang.resolve(), options.upstream.resolve())
