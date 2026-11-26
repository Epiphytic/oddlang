"""List native modules still outside the two closed Rust import profiles."""
import argparse
from collections import defaultdict
import gzip
import json
from pathlib import Path


FRAME = "crates/marmish-core/src/frame.rs"
FACADE = "crates/marmish-core/src/lib.rs"
# Representative, independently executable examples to add when a profile expands.
TASKS = {
    "modules": ("public module and re-export resolution", "two modules re-export a checked type",
                "public path and type identity agree", "reject an ambiguous or cfg-dependent re-export"),
    "config": ("URL normalization and configuration validators", "a two-entry relay list",
               "valid inputs have the same normalized values", "reject invalid URLs with the same first error"),
    "wire": ("numbered Rust enum tags and checked binary cursors", "a one-tag record with bounded length",
             "encode and decode exact original bytes", "reject unknown tags and truncated input"),
    "offset": ("bounded offsets and ordered window transitions", "a checkpoint with one byte pending",
               "resume at the admissible offset", "preserve the original error on a gap or overflow"),
    "compression": ("bounded external compression primitive", "a length-limited block codec",
                    "match decompressed bytes and output bounds", "reject an invalid or oversized block"),
    "native": ("general Rust bodies and native dependency resolution", "a public function from the listed file isolated with its resolved imports",
               "compare the reduced entry point and all observable state with the pinned original", "reject an unresolved import, effect or expansion"),
    "os": ("platform-specific library calls and observable OS effects", "a local fixture with a capability supplied by the caller",
           "replay declared local outcomes on the named platform", "reject an undeclared capability"),
}


def family(path):
    components = Path(path).parts
    if len(components) < 3 or components[0] != "crates":
        raise ValueError(f"runtime source has no known first-party package: {path}")
    crate = components[1]
    if path.endswith("/lib.rs") and crate == "marmish-core":
        return "modules"
    if path == "crates/marmish-core/src/config.rs":
        return "config"
    if crate == "marmish-stream":
        return {"codec.rs": "compression", "windows.rs": "offset", "wire.rs": "wire",
                "copy_protocol.rs": "wire"}.get(path.rsplit("/", 1)[1], "native")
    return "os" if crate == "marmish-vault" else "native"


def generate(inventory, generated=(FRAME, FACADE)):
    paths = defaultdict(list)
    for symbol in inventory["symbols"]:
        if symbol["classification"] == "retained-native" and symbol["path"] not in generated:
            paths[symbol["path"]].append(symbol)
    needed = {family(path) for path in paths}
    scope = "the framing module" if set(generated) == {FRAME} else "the regenerated modules"
    return {
        "revision": inventory["gitRevision"],
        "scope": f"Known runtime-source lexical declaration sites outside {scope}. Macro expansions, lexical call targets and per-configuration compiled-item counts remain open.",
        "tasks": [{"id": "urn:oddlang:work:marmish-capability-" + key, "construct": task[0],
                   "fixtureToExtract": task[1], "positive": task[2], "negative": task[3]}
                  for key, task in TASKS.items() if key in needed],
        "modules": [{"path": path, "lexicalSites": len(symbols),
                     "targets": sorted({t for s in symbols for t in s["targets"]}),
                     "status": "retained-native",
                     "blockedOn": "urn:oddlang:work:marmish-capability-" + family(path)}
                    for path, symbols in sorted(paths.items())],
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inventory", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--framing-only", action="store_true", help="Reproduce the Phase 6 inventory boundary")
    args = parser.parse_args()
    with gzip.open(args.inventory) as handle:
        result = generate(json.load(handle), (FRAME,) if args.framing_only else (FRAME, FACADE))
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
