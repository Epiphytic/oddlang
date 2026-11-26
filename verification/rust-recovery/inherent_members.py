"""Extract public names from concrete local inherent implementations in Rustdoc JSON."""

from collections import Counter


def inventory(document, owners):
    index = {row["id"]: row for row in document["index"].values()}
    excluded = Counter()
    declarations = []
    for row in index.values():
        impl = row["inner"].get("impl")
        if row["crate_id"] != 0 or impl is None or impl["trait"] is not None:
            continue
        receiver = impl["for"].get("resolved_path")
        if receiver is None or receiver["id"] not in owners:
            excluded["unresolved-or-nonnominal-receiver"] += 1
            continue
        owner = index[receiver["id"]]
        nominal = next(iter(owner["inner"].values()))
        if (receiver["args"] is not None or nominal["generics"]["params"]
                or impl["generics"]["params"] or impl["generics"]["where_predicates"]):
            excluded["generic-or-specialized-receiver"] += 1
            continue
        for item_id in impl["items"]:
            item = index[item_id]
            if item["visibility"] != "public":
                continue
            kind = next(iter(item["inner"]))
            if kind not in {"function", "assoc_const", "assoc_type"} or not item["name"]:
                raise ValueError(f"unsupported public inherent item kind or name: {kind}")
            declarations.append({"owner": owners[receiver["id"]], "kind": kind, "name": item["name"]})
    declarations.sort(key=lambda item: (item["owner"], item["kind"], item["name"]))
    return {"items": declarations, "excludedLocalInherentImpls": dict(sorted(excluded.items()))}
