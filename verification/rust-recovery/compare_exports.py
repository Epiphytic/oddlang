"""Summarize observable public type differences across two native packages."""

import argparse
import json
from pathlib import Path

from probe_exports import bindings, sha


def observed(path):
    raw = path.read_bytes()
    graph = json.loads(raw)
    edges = bindings(graph, "bindings")

    def audience(field):
        return field["audience"]["kind"]

    def fields(definition):
        return {
            **{name: audience(field) for name, field in definition["fields"].items()},
            **{
                f"{variant}::{name}": audience(field)
                for variant, row in definition["variants"].items()
                for name, field in row["fields"].items()
            },
        }

    def row(edge):
        data = graph["data"].get(edge["target"])
        return {"kind": edge["kind"], "fieldAudiences": fields(data) if data else {}}

    equivalence = {}
    for name, edge in edges.items():
        if edge["kind"] != "primitive":
            equivalence.setdefault(edge["target"], []).append(name)
    aliases = {
        alias: tuple(sorted(family))
        for family in equivalence.values()
        for alias in family
    }
    return {name: row(edge) for name, edge in edges.items()}, aliases, sha(raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original", type=Path)
    parser.add_argument("generated", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    old, old_alias, old_hash = observed(args.original)
    new, new_alias, new_hash = observed(args.generated)
    same = old.keys() & new.keys()
    shared = []
    for name in sorted(same):
        before = old[name]
        after = new[name]
        if before != after or old_alias.get(name) != new_alias.get(name):
            shared.append(
                {
                    "spelling": name,
                    "original": before,
                    "generated": after,
                    "originalAliases": old_alias.get(name),
                    "generatedAliases": new_alias.get(name),
                }
            )
    result = {
        "originalSha256": old_hash,
        "generatedSha256": new_hash,
        "profiles": "public module type edges and field reading at the canonical cut",
        "sharedCount": len(same),
        "originalOnly": sorted(old.keys() - new.keys()),
        "generatedOnly": sorted(new.keys() - old.keys()),
        "changedAtSharedSpelling": shared,
    }
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(
        len(same),
        len(result["originalOnly"]),
        len(result["generatedOnly"]),
        len(shared),
    )


if __name__ == "__main__":
    main()
