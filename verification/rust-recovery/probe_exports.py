"""Verify compiler-export candidates against rustdoc JSON and downstream rustc."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import heapq
import json
import os
from pathlib import Path
import subprocess
import tempfile

PIN = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
DOC_TYPES = {
    "module",
    "struct",
    "enum",
    "union",
    "trait",
    "trait_alias",
    "type_alias",
    "variant",
    "primitive",
    "extern_crate",
}
ADAPTER = [
    "Cargo.toml",
    "Cargo.lock",
    "driver.rs",
    "shapes.rs",
    "shape_model.rs",
    "exports.rs",
    "export_model.rs",
]


def invoke(args, *, env=None, expect=True, cwd=None):
    result = subprocess.run(args, env=env, cwd=cwd, text=True, capture_output=True, timeout=1800)
    if expect and result.returncode:
        raise RuntimeError(result.stderr[-15000:])
    return result


def pinned_environment():
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(
            (
                "RUST",
                "CARGO_ENCODED_",
                "CARGO_PROFILE_",
                "CARGO_BUILD_RUST",
                "CARGO_TARGET_",
            )
        )
        and key not in ("CARGO_BUILD_TARGET", "CARGO_TARGET_DIR")
    }
    env["RUSTUP_TOOLCHAIN"] = "1.97.1"
    env["CARGO_NET_OFFLINE"] = "true"
    return env


def sha(data):
    return hashlib.sha256(data).hexdigest()


def profile(manifest, workspace, cli_name, args, env):
    metadata = json.loads(
        invoke(
            [
                "cargo",
                "+1.97.1",
                "metadata",
                "--offline",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                manifest,
            ],
            env=env,
        ).stdout
    )
    lock = Path(metadata["workspace_root"]) / "Cargo.lock"
    args.workspace = metadata["workspace_root"]
    values = dict(
        adapter=str(Path(__file__).resolve().with_name("run.sh")),
        adapterInputs=ADAPTER,
        package=args.package,
        targetKind="lib",
        targetName=cli_name,
        targetTriple=args.target,
        features=args.feature,
        defaultFeatures=args.default_features,
        compilerCommit=PIN,
        cargoLockSha256=sha(lock.read_bytes()),
    )
    p = workspace / "profile.toml"
    p.write_text("".join(f"{key} = {json.dumps(val)}\n" for key, val in values.items()))
    return p


def spelling(parts):
    return "::".join(
        part if part in ("crate", "self", "super", "Self") else "r#" + part
        for part in parts
    )


def compile_probe(lines, cwd, dependency, env, *, lint="", expect=True):
    caller = cwd / "caller.rs"
    caller.write_text(lint + "\n" + "\n".join(lines) + "\n")
    return invoke(
        [
            "rustc",
            "+1.97.1",
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit=metadata",
            "--target",
            dependency.parents[2].name,
            "--error-format=json",
            "--crate-name",
            "separate_caller",
            "--out-dir",
            cwd,
            "--extern",
            "observed=" + str(dependency),
            "-L",
            "dependency=" + str(dependency.parent),
            "-L",
            "dependency=" + str(dependency.parents[3] / "release" / "deps"),
            caller,
        ],
        env=env,
        expect=expect,
    )


def bindings(graph, table):
    return {
        (tuple(row["path"]) + (name,)): value
        for row in graph["modules"].values()
        for name, value in row[table].items()
    }


def rustdoc_exports(doc):
    index = doc["index"]
    paths = doc["paths"]

    def get_item(ident):
        return index.get(str(ident))

    def kind(ident):
        item = get_item(ident)
        return (
            next(iter(item["inner"])) if item else paths.get(str(ident), {}).get("kind")
        )

    # Rustdoc describes uses separately, not an already resolved module
    # table. Resolve public named items first and then saturate glob edges.
    modules = {}
    imports = {}
    for item in index.values():
        module = item["inner"].get("module")
        if module is None or item["crate_id"] != 0:
            continue
        named, globs = {}, set()
        for ident in module["items"]:
            child = get_item(ident)
            if child is None or child["visibility"] != "public":
                continue
            record = child["inner"].get("use")
            if record is not None:
                target = record["id"]
                if record["is_glob"]:
                    if target is not None:
                        globs.add(target)
                    continue
                name = record["name"]
            else:
                name, target = child["name"], ident
            if name not in (None, "_") and kind(target) in DOC_TYPES:
                named.setdefault(name, set()).add(target)
        modules[item["id"]] = named
        imports[item["id"]] = globs

    explicit = {key: set(row) for key, row in modules.items()}
    ignored = set()
    changed = True
    while changed:
        changed = False
        for key, globs in imports.items():
            for target in globs:
                if target in modules:
                    choices = list(modules[target].items())
                elif kind(target) == "enum" and get_item(target) is not None:
                    choices = [
                        (get_item(variant)["name"], {variant})
                        for variant in get_item(target)["inner"]["enum"]["variants"]
                        if get_item(variant)
                    ]
                else:
                    ignored.add((key, target))
                    continue
                for name, values in choices:
                    if name in explicit[key]:
                        continue
                    current = modules[key].setdefault(name, set())
                    updated = current | values
                    if updated != current:
                        modules[key][name] = updated
                        changed = True
    queue = [(0, (), doc["root"])]
    canonical, exported = {}, {}
    while queue:
        length, path, key = heapq.heappop(queue)
        if key in canonical:
            continue
        canonical[key] = path
        for name, targets in modules[key].items():
            edge = path + (name,)
            exported[edge] = [
                {
                    "kind": kind(target),
                    "path": paths.get(str(target), {}).get("path"),
                    "target": target,
                }
                for target in sorted(targets)
            ]
            if len(targets) == 1:
                target = next(iter(targets))
                if target in modules:
                    heapq.heappush(queue, (length + 1, edge, target))
    unresolved_globs = [
        {
            "module": list(canonical[module]),
            "target": paths.get(str(target), {}).get("path"),
        }
        for module, target in sorted(ignored)
        if module in canonical
    ]
    return exported, unresolved_globs


def probe_fields(graph, exported, documented, doc, cwd, dep, env):
    paths = {}
    for name, edge in exported.items():
        if edge["kind"] == "data":
            paths.setdefault(edge["target"], []).append(name)
    positive, negative, gaps = [], [], []
    for target, data in graph["data"].items():
        candidates = sorted(paths.get(target, ()), key=lambda name: (len(name), name))
        selected = None
        for name in candidates:
            for row in documented.get(name, []):
                item = doc["index"].get(str(row["target"]))
                if item and row["kind"] in ("struct", "union", "enum"):
                    selected = (name, row["kind"], item["inner"][row["kind"]])
                    break
            if selected:
                break
        if not selected:
            gaps.append(
                {"target": target, "reason": "rustdoc nominal definition absent"}
            )
            continue
        name, kind, declaration = selected
        if declaration.get("generics", {}).get("params"):
            gaps.append(
                {
                    "target": target,
                    "reason": "generic field receiver requires explicit bounds",
                }
            )
            continue
        receiver = spelling(("observed",) + name)

        def add_expression(expression, public):
            sequence = positive if public else negative
            sequence.append(
                f"fn read_{len(sequence)}(value: &{receiver}) {{ {expression} }}"
            )

        for field, record in data["fields"].items():
            native = field if field.isdecimal() else spelling((field,))
            access = f"let _ = &value.{native};"
            if kind == "union":
                access = "unsafe { " + access + " }"
            add_expression(access, record["audience"]["kind"] == "public")
        for variant, detail in data["variants"].items():
            fields = detail["fields"]
            for field, record in fields.items():
                constructor = receiver + "::" + spelling((variant,))
                if field.isdecimal():
                    prefix = ", ".join(["_"] * int(field) + ["member", ".."])
                    destructure = f"{constructor}({prefix})"
                else:
                    destructure = (
                        f"{constructor} {{ {spelling((field,))}: member, .. }}"
                    )
                add_expression(
                    f"if let {destructure} = value {{ let _ = member; }}",
                    record["audience"]["kind"] == "public",
                )
    if positive:
        compile_probe(positive, cwd, dep, env)
    for code in negative:
        observed = compile_probe([code], cwd, dep, env, expect=False)
        diagnostics = [
            json.loads(line)
            for line in observed.stderr.splitlines()
            if line.startswith("{")
        ]
        assert observed.returncode and any(
            (item.get("code") or {}).get("code") == "E0616" for item in diagnostics
        ), observed.stderr
    return {
        "publicReads": len(positive),
        "privateRejections": len(negative),
        "gaps": gaps,
    }


def qualification(args):
    env = pinned_environment()
    version = invoke(["rustc", "+1.97.1", "-vV"], env=env).stdout
    assert f"commit-hash: {PIN}\n" in version
    if args.target is None:
        args.target = next(
            line.split()[1]
            for line in version.splitlines()
            if line.startswith("host: ")
        )
    host = next(
        line.split()[1] for line in version.splitlines() if line.startswith("host: ")
    )
    if args.target != host:
        raise ValueError(
            "independent field and import probes require the matching native target"
        )
    manifest = args.manifest.resolve()
    cli_name = args.target_name or args.package.replace("-", "_")
    options = [
        "--locked",
        "--offline",
        "--release",
        "--manifest-path",
        str(manifest),
        "--package",
        args.package,
        "--target",
        args.target,
        "--lib",
    ]
    if not args.default_features:
        options.append("--no-default-features")
    for feature in args.feature:
        options.extend(["--features", feature])
    # Use separate target dirs. rustdoc doesn't inspect the recovery candidate.
    args.out.mkdir(parents=True, exist_ok=False)
    out = args.out.resolve()
    with tempfile.TemporaryDirectory(prefix="type-probes-") as scratch:
        scratch = Path(scratch)
        p = profile(manifest, scratch, cli_name, args, env)
        command = [
            args.oddlang.resolve(),
            "recover",
            "rust-exports",
            manifest,
            "--profile",
            p,
        ]
        target_dir = out / "native"
        with ThreadPoolExecutor(max_workers=4) as executor:
            first = executor.submit(
                invoke, command + ["--out", out / "recovered"], env=env
            )
            second = executor.submit(
                invoke, command + ["--out", scratch / "recovered-again"], env=env
            )
            native = executor.submit(
                invoke,
                [
                    "cargo",
                    "+1.97.1",
                    "check",
                    *options,
                    "--target-dir",
                    target_dir,
                    "--message-format=json",
                ],
                env=env,
                cwd=manifest.parent,
            )
            doc_target = out / "rustdoc"
            rustdoc = executor.submit(
                invoke,
                [
                    "cargo",
                    "+1.97.1",
                    "rustdoc",
                    *options,
                    "--target-dir",
                    doc_target,
                    "--",
                    "-Z",
                    "unstable-options",
                    "--output-format",
                    "json",
                ],
                env={**env, "RUSTC_BOOTSTRAP": "1"},
                cwd=manifest.parent,
            )
            first.result()
            second.result()
            native = native.result()
            rustdoc.result()
        for file in ("exports.json", "exports.lock"):
            assert (out / "recovered" / file).read_bytes() == (
                scratch / "recovered-again" / file
            ).read_bytes()
        source = (out / "recovered/exports.json").read_bytes()
        graph = json.loads(source)
        graph_bindings = bindings(graph, "bindings")
        ambiguous = bindings(graph, "ambiguous")
        dependency = next(
            Path(file)
            for line in native.stdout.splitlines()
            for row in [json.loads(line)]
            if row.get("reason") == "compiler-artifact"
            and row.get("target", {}).get("name") == cli_name
            for file in row["filenames"]
            if file.endswith(".rmeta")
        )
        imports = [
            f"pub use {spelling(('observed',) + edge)} as witness_{i};"
            for i, edge in enumerate(sorted(graph_bindings))
        ]
        compile_probe(
            imports, scratch, dependency, env, lint="#![deny(ambiguous_glob_imports)]"
        )
        doc_target = out / "rustdoc"
        doc = json.loads(
            (doc_target / args.target / "doc" / (cli_name + ".json")).read_bytes()
        )
        documented, unresolved_globs = rustdoc_exports(doc)
        differences = []
        for edge in sorted(
            graph_bindings.keys() | documented.keys() | ambiguous.keys()
        ):
            observed = graph_bindings.get(edge)
            listed = documented.get(edge, [])
            recorded = {("::".join(row["path"])) for row in listed if row["path"]}
            target = (
                observed["target"].split("::", 1)[-1].replace("::{type}", "")
                if observed
                else None
            )
            if edge in ambiguous:
                disposition = "compiler-ambiguity"
            elif observed is None:
                result = compile_probe(
                    [f"pub use {spelling(('observed',) + edge)} as witness;"],
                    scratch,
                    dependency,
                    env,
                    expect=False,
                )
                if result.returncode == 0:
                    disposition = "rustdoc-only-resolves"
                else:
                    errors = [
                        json.loads(line).get("message")
                        for line in result.stderr.splitlines()
                        if line.startswith("{")
                    ]
                    disposition = {"rustdoc-only-rejected": errors}
            elif not listed:
                disposition = "compiler-and-caller-only"
            elif target not in recorded:
                disposition = "both-resolve-origin-path-difference"
            else:
                continue
            differences.append(
                {
                    "spelling": list(edge),
                    "disposition": disposition,
                    "compiler": observed or ambiguous.get(edge),
                    "rustdoc": listed,
                }
            )
        access = probe_fields(
            graph, graph_bindings, documented, doc, scratch, dependency, env
        )
        receipt = {
            "compiler": PIN,
            "target": args.target,
            "package": args.package,
            "features": args.feature,
            "defaultFeatures": args.default_features,
            "exportsSha256": sha(source),
            "moduleCount": len(graph["modules"]),
            "compilePositiveCount": len(imports),
            "rustdocSpellingCount": len(documented),
            "rustdocFormat": doc["format_version"],
            "ambiguityCount": len(ambiguous),
            "rustdocUnresolvedGlobs": unresolved_globs,
            "unresolvedCompilerSpellings": sorted(
                tuple(row["path"]) + (name,)
                for row in graph["modules"].values()
                for name in row["unresolved"]
            ),
            "discrepancies": differences,
            "deterministic": True,
            "fieldProbes": access,
        }
        (out / "evidence.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n"
        )
        print(args.package, len(imports), len(documented), len(differences))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("oddlang", type=Path)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("package")
    parser.add_argument("out", type=Path)
    parser.add_argument("--target-name")
    parser.add_argument("--target")
    parser.add_argument("--feature", action="append", default=[])
    parser.add_argument("--default-features", action="store_true")
    qualification(parser.parse_args())


if __name__ == "__main__":
    main()
