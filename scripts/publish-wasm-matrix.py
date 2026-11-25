"""Publish the Phase 1 compatibility fixtures and the independent host-binding probe."""

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def publish(binary, out, name, original, edit):
    with tempfile.TemporaryDirectory() as temp:
        source = Path(temp) / "source"
        shutil.copytree(Path("contracts/wasm") / original, source)
        model = source / "model.ttl"
        cases = source / "corpus.json"
        turtle, corpus = edit(model.read_text(), json.loads(cases.read_bytes()))
        model.write_text(turtle)
        cases.write_text(json.dumps(corpus, ensure_ascii=False, indent=2) + "\n")
        subprocess.run([binary, "wasm", "publish", source,
                        "contracts/wasm/empty.lock.json", "contracts/cache", out / name],
                       check=True)


def label(turtle, corpus):
    return turtle + '\ne:echo w:label "Internal description" .\n', corpus


def rename(turtle, corpus):
    for call in corpus[0]["calls"]:
        call["function"] = "mirror"
    return turtle.replace('w:name "echo"', 'w:name "mirror"'), corpus


def tags(turtle, corpus):
    for name, old, new in [("empty", 0, 1), ("number", 1, 0)]:
        turtle = turtle.replace(f'w:name "{name}" ; w:ordinal {old}',
                                f'w:name "{name}" ; w:ordinal {new}')
    return turtle, corpus


def required_import(turtle, corpus):
    turtle += '\ne:world w:import e:seed .\ne:seed a w:Function ; w:name "seed" ; w:result "s64" .\n'
    corpus[0]["covers"].append({"declaration": "urn:oddlang:echo:seed", "role": "wasm.importer"})
    corpus[0]["imports"] = [{"function": "seed", "arguments": [], "returns": "1"}]
    return turtle, corpus


def probe(turtle, _corpus):
    return turtle, json.loads(Path("examples/wasm/claims/probe-corpus.json").read_bytes())


if __name__ == "__main__":
    binary, out = sys.argv[1:]
    for suffix, edit in [("label", label), ("rename", rename), ("tags", tags),
                         ("import", required_import)]:
        publish(binary, Path(out), f"echo-{suffix}", "echo", edit)
    publish(binary, Path(out), "ledger-probe", "ledger", probe)
