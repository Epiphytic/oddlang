#!/usr/bin/env python3
"""Compare named workspace declarations in raw compiler and Rustdoc inventories."""
import argparse
import json
from pathlib import Path

KINDS = {
    "associated-function": "function",
    "associated-type": "assoc_type",
    "constant": "constant",
    "enum": "enum",
    "field": "struct_field",
    "function": "function",
    "macro": "macro",
    "module": "module",
    "struct": "struct",
    "type-alias": "type_alias",
    "variant": "variant",
}


def read(path):
    return json.loads(path.read_bytes())


def keys(source, root, include_macros):
    for row in source["declarations"]:
        kind = KINDS.get(row["kind"])
        if kind is None or not row["file"] or not row["line"] or not row["name"]:
            continue
        if row["macroGenerated"] and not include_macros:
            continue
        path = Path(row["file"])
        if path.is_absolute():
            try:
                path = path.relative_to(root)
            except ValueError:
                continue
        yield path.as_posix(), row["line"], row["name"], kind


def doc_keys(source):
    for row in source["definitions"]:
        if not row["span"] or not row["name"] or row["kind"] not in KINDS.values():
            continue
        span = row["span"]
        scope, path = span["file"]
        if scope == "workspace":
            yield path, span["begin"][0], row["name"], row["kind"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler_inventory", type=Path)
    parser.add_argument("rustdoc_inventory", type=Path)
    args = parser.parse_args()
    compiler = read(args.compiler_inventory)
    rustdoc = read(args.rustdoc_inventory)
    roots = {row["path"][0] for row in rustdoc["definitions"] if row.get("path")}
    if compiler["crateName"] not in roots:
        parser.error("Rustdoc has no path rooted at the compiler's crate name")
    expected = set(doc_keys(rustdoc))
    root = Path(rustdoc["workspaceRoot"])
    scopes = {}
    for name, include_macros in [("ordinary", False), ("includingExpansions", True)]:
        actual = set(keys(compiler, root, include_macros))
        scopes[name] = {
            "matched": len(actual & expected),
            "compilerOnly": sorted(actual - expected),
            "rustdocOnly": sorted(expected - actual),
        }
    print(json.dumps({
        "scope": "Declaration overlap by workspace path, line, name and normalized kind",
        "compilerCommit": compiler["compiler"],
        "rustdocCompiler": rustdoc["compiler"],
        "crate": compiler["crateName"],
        "matchByScope": scopes,
    }, indent=2))


if __name__ == "__main__":
    main()
