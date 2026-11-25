"""Collect Rust module edges and conditional ranges with Marmish's locked parser."""

import json
import re
import sys
from pathlib import Path

from tree_sitter import Language, Parser
import tree_sitter_rust


def decode(node):
    return node.text.decode()


def inspect(parser, data):
    edges, ranges = [], []

    def walk(node, inline):
        attrs = []
        for child in node.named_children:
            if child.type in ("line_comment", "block_comment"):
                continue
            if child.type == "attribute_item":
                attrs.append(decode(child))
                continue
            cfg = [
                a
                for a in attrs
                if re.match(r"#\[\s*(cfg|cfg_attr|test|ignore)\b", a)
                or a.startswith("#[tokio::test")
            ]
            if cfg:
                ranges.append([child.start_point.row + 1, child.end_point.row + 1, cfg])
            inner = inline
            if child.type == "mod_item":
                name = decode(child.child_by_field_name("name"))
                if child.child_by_field_name("body") is None:
                    paths = [
                        m[1]
                        for a in attrs
                        if (m := re.fullmatch(r'#\[path\s*=\s*"([^"]+)"\]', a))
                    ]
                    edges.append(
                        {
                            "kind": "mod",
                            "name": name,
                            "path": next(iter(paths), None),
                            "inline": inline,
                            "line": child.start_point.row + 1,
                        }
                    )
                else:
                    inner = [*inline, name]
            if child.type == "macro_invocation":
                macro = child.child_by_field_name("macro")
                if macro and decode(macro) == "include":
                    m = re.fullmatch(r'include!\s*\(\s*"([^"]+)"\s*\)', decode(child))
                    edges.append(
                        {
                            "kind": "include",
                            "path": m[1] if m else None,
                            "line": child.start_point.row + 1,
                        }
                    )
            if child.named_child_count:
                walk(child, inner)
            attrs = []

    walk(parser.parse(data).root_node, [])
    return {"edges": edges, "attributes": ranges}


if __name__ == "__main__":
    root = Path(sys.argv[1])
    parser = Parser(Language(tree_sitter_rust.language()))
    json.dump(
        {p: inspect(parser, (root / p).read_bytes()) for p in json.load(sys.stdin)},
        sys.stdout,
    )
