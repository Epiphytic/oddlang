#!/usr/bin/env python3
"""Reconcile declaration names observed by the release compiler and same-toolchain rustdoc."""
import argparse
from collections import Counter, defaultdict
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

import reconcile_rustdoc as historical

HERE = Path(__file__).resolve().parent
TOOLS = "1.97.1"
COMMIT = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
spec = importlib.util.spec_from_file_location("cargo_trace", HERE.parent / "rust-inventory/inventory.py")
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)

COMPILER_KINDS = {
    "crate": "package", "module": "module", "struct": "record", "union": "union",
    "enum": "sum", "variant": "variant", "field": "field", "trait": "interface", "macro": "macro",
    "function": "operation", "associated-function": "operation", "associated-type": "associated-type",
    "type-alias": "type-alias", "constant": "constant", "associated-constant": "constant", "static": "static",
}
DOC_KINDS = {
    "module": "module", "struct": "record", "union": "union", "enum": "sum", "variant": "variant",
    "struct_field": "field", "trait": "interface", "macro": "macro", "function": "operation",
    "assoc_type": "associated-type", "type_alias": "type-alias", "constant": "constant",
    "assoc_const": "constant", "static": "static",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def tool(name):
    return Path(subprocess.check_output(["rustup", "which", "--toolchain", TOOLS, name], text=True).strip())


def tool_identity():
    result = {}
    for name in ("rustc", "rustdoc"):
        version = subprocess.check_output([tool(name), "-vV"], text=True)
        fields = dict(line.split(": ", 1) for line in version.splitlines() if ": " in line)
        require(fields["commit-hash"] == COMMIT and fields["release"] == TOOLS, f"unexpected {name} release")
        if result:
            require(result["host"] == fields["host"], "toolchain hosts differ")
        result.update(host=fields["host"], commit=COMMIT, release=TOOLS)
        result[name + "Sha256"] = trace.sha(tool(name))
    return result


def command(args, cwd, env):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True)
    require(result.returncode == 0, f"{args[0]} failed: {result.stderr.decode()[-3000:]}")
    return result.stdout, result.stderr.decode()


def invocation(log, executable, crate):
    selected = []
    for words in trace.commands(log):
        if str(executable) in words:
            args = words[words.index(str(executable)) + 1:]
            if trace.flag(args, "--crate-name") == [crate]:
                selected.append(args)
    require(len(selected) == 1, f"expected one {executable.name} invocation for {crate}, found {len(selected)}")
    return selected[0]


def profile(args, root):
    # Cargo gives rustdoc its own metadata hash and output paths. Preserve
    # target CPU/features, panic strategy, optimization and assertion flags.
    codegen = [flag for flag in trace.flag(args, "-C")
               if flag.split("=", 1)[0] not in {"metadata", "extra-filename", "embed-bitcode", "strip", "debuginfo",
                                                 "split-debuginfo", "linker", "incremental"}]
    return {
        "edition": [word for word in args if word.startswith("--edition=")],
        "crateType": trace.flag(args, "--crate-type"), "target": trace.flag(args, "--target"),
        "crateName": trace.flag(args, "--crate-name"), "explicitCfg": sorted(trace.flag(args, "--cfg")),
        "checkCfg": sorted(trace.flag(args, "--check-cfg")), "codegen": codegen,
        "externAliases": sorted(x.split("=",1)[0] for x in trace.flag(args, "--extern")),
        "pathMaps": [word.replace(str(root), "$WORKSPACE") for word in args if word.startswith("--remap-path-prefix=")],
    }


def cargo_packages(root, triple, package, no_default, features, env):
    flags = [tool("cargo"), "metadata", "--format-version", "1", "--locked", "--offline",
             "--filter-platform", triple]
    if no_default:
        flags.append("--no-default-features")
    for feature in features:
        flags.extend(["--features", feature if "/" in feature else f"{package}/{feature}"])
    raw, _ = command(flags, root, env)
    packages = {}
    for entry in json.loads(raw)["packages"]:
        directory = Path(entry["manifest_path"]).resolve().parent
        if entry["source"] is None:
            identity = {"path": os.path.relpath(directory, root)}
        else:
            identity = {"source": entry["source"]}
        packages[directory] = {**identity, "name": entry["name"], "version": entry["version"]}
    return packages


def extern_bindings(log, rustc, selected, packages):
    """Resolve each selected --extern file back to its producer in the same fresh build."""
    producers = defaultdict(list)
    for words in trace.commands(log):
        if str(rustc) not in words:
            continue
        index = words.index(str(rustc))
        args = words[index + 1:]
        env = dict(word.split("=", 1) for word in words[:index] if "=" in word)
        crate_types = trace.flag(args, "--crate-type")
        if not set(crate_types).intersection({"lib", "rlib", "dylib", "proc-macro"}):
            continue
        crate, = trace.flag(args, "--crate-name")
        output, = trace.flag(args, "--out-dir")
        suffix, = [v.removeprefix("extra-filename=") for v in trace.flag(args, "-C")
                   if v.startswith("extra-filename=")]
        directory = Path(env["CARGO_MANIFEST_DIR"]).resolve()
        require(directory in packages, f"unknown Cargo package producing {crate}")
        target = trace.flag(args, "--target")
        evidence = {"package": packages[directory], "crate": crate, "crateTypes": sorted(crate_types),
                    "target": target, "explicitCfg": sorted(trace.flag(args, "--cfg"))}
        producers[(Path(output), crate + suffix)].append(evidence)

    bindings = {}
    for arg in trace.flag(selected, "--extern"):
        alias, sep, filename = arg.partition("=")
        require(sep and alias not in bindings, f"missing or duplicate extern binding: {alias}")
        artifact = Path(filename)
        require(artifact.suffix in {".rmeta", ".rlib", ".dylib", ".so", ".dll"},
                f"unsupported extern artifact: {artifact.name}")
        stems = {artifact.stem, artifact.stem.removeprefix("lib")}
        candidates = [row for stem in stems for row in producers.get((artifact.parent, stem), [])]
        require(len(candidates) == 1, f"expected one build producer for extern {alias}, found {len(candidates)}")
        bindings[alias] = candidates[0]
    return bindings


def compare_profiles(left, right):
    for key in left:
        require(left[key] == right.get(key), f"observer profile changed {key}: {left[key]} vs {right.get(key)}")


def members(kind, data):
    if kind in {"module", "trait", "impl"}:
        return data["items"]
    if kind == "enum":
        return data["variants"]
    if kind == "union":
        return data["fields"]
    if kind in {"struct", "variant"} and isinstance(data["kind"], dict):
        form, payload = next(iter(data["kind"].items()))
        if form == "tuple":
            return [item for item in payload if item is not None]
        return payload["fields"]
    return []


def compiler_rows(inventory):
    for row in inventory["declarations"]:
        yield row["id"], row["owner"], row["name"], row["kind"], COMPILER_KINDS.get(row["kind"]), row["file"], row["line"]


def doc_rows(document):
    rows = {item["id"]: item for item in document["index"].values() if item["crate_id"] == 0}
    parents = defaultdict(set)
    for owner, row in rows.items():
        kind, value = next(iter(row["inner"].items()))
        for child in members(kind, value):
            parents[child].add(owner)
    for key, row in rows.items():
        kind = next(iter(row["inner"]))
        relative = row.get("span") or {}
        owner = next(iter(parents[key])) if len(parents[key]) == 1 else None
        normalized = "package" if key == document["root"] else DOC_KINDS.get(kind)
        yield key, owner, row["name"], kind, normalized, relative.get("filename"), (relative.get("begin") or [None])[0]


def observations(rows, root):
    rows = {row[0]: row for row in rows}
    mapped = defaultdict(list)
    omitted = Counter()
    for key, parent, name, original_kind, kind, file, line in rows.values():
        path = []
        current = key
        while current in rows:
            identity, current, segment, _, category, _, _ = rows[current]
            if not segment or not category:
                break
            path.append(segment)
            if category == "package":
                name_key = tuple(reversed(path)), kind
                if file:
                    file = str(file).removeprefix("/oddlang-workspace/").removeprefix(str(root) + "/")
                mapped[name_key].append({"file": file, "line": line})
                break
        else:
            category = None
        if not path or category != "package":
            omitted[original_kind] += 1
    return mapped, dict(sorted(omitted.items()))


def compare(compiler, document, root):
    left, left_skipped = observations(compiler_rows(compiler), root)
    right, right_skipped = observations(doc_rows(document), root)
    groups = {key: [] for key in ("matched", "compilerOnly", "rustdocOnly", "ambiguous")}
    for key in sorted(left.keys() | right.keys()):
        comp, doc = left.get(key, []), right.get(key, [])
        group = ("ambiguous" if len(comp) > 1 or len(doc) > 1 else
                 "matched" if comp and doc else "compilerOnly" if comp else "rustdocOnly")
        groups[group].append({"path": list(key[0]), "kind": key[1], "compiler": comp, "rustdoc": doc})
    return dict(groups, unkeyedCompilerByKind=left_skipped, unkeyedRustdocByKind=right_skipped)


def span_evidence(compiler, document, root, previous):
    """Preserve the old span-based metric separately from named identity."""
    docs = set()
    for item in document["index"].values():
        kind = next(iter(item["inner"]))
        if item["crate_id"] or not item["name"] or not item["span"] or kind not in historical.KINDS.values():
            continue
        span = item["span"]
        path = Path(span["filename"].removeprefix("/oddlang-workspace/"))
        if path.is_absolute():
            if not path.is_relative_to(root):
                continue
            path = path.relative_to(root)
        docs.add((path.as_posix(), span["begin"][0], item["name"], kind))
    scope = {}
    for name, macros in (("ordinary", False), ("includingExpansions", True)):
        compiled = set(historical.keys(compiler, root, macros))
        scope[name] = {"matched": len(docs & compiled), "compilerOnly": sorted(compiled - docs),
                       "rustdocOnly": sorted(docs - compiled)}
        if previous:
            prior = json.loads(previous.read_bytes())
            require(prior["compilerCommit"] == COMMIT and prior["crate"] == compiler["crateName"],
                    "historical report toolchain or target mismatch")
            revisited = []
            for side in ("compilerOnly", "rustdocOnly"):
                for row in prior["matchByScope"][name][side]:
                    key = tuple(row)
                    state = ("matched" if key in docs and key in compiled else "compilerOnly" if key in compiled
                             else "rustdocOnly" if key in docs else "absent")
                    revisited.append({"key": key, "previous": side, "current": state})
            scope[name]["priorKeys"] = revisited
    return scope


def pinned_source(root, revision):
    if revision is None:
        return None
    result = subprocess.check_output(["git", "rev-parse", "HEAD", "HEAD^{tree}"], cwd=root, text=True).splitlines()
    require(result[0] == revision, "source revision differs from pin")
    require(not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=root),
            "source checkout must be clean")
    return {"revision": result[0], "tree": result[1]}


def collect(root, package, kind, target, triple, no_default, features, previous=None, revision=None):
    root = root.resolve()
    source = pinned_source(root, revision)
    manifest = root / "Cargo.toml"
    crate = target.replace("-", "_")
    tools = tool_identity()
    # Restrict inherited build overrides equally for both observers.
    env = {k: os.environ[k] for k in ("HOME", "PATH", "CARGO_HOME", "RUSTUP_HOME", "TMPDIR", "SDKROOT")
           if k in os.environ}
    env["LC_ALL"] = "C"
    rustc = tool("rustc")
    rustdoc = tool("rustdoc")
    with tempfile.TemporaryDirectory(prefix="oddlang-release-doc-") as directory:
        build = Path(directory)
        raw, compiler_log = command([HERE / "run.sh", manifest, package, kind, target, triple,
                                     "no-default" if no_default else "default", *features],
                                    root, env | {"ODD_RECOVERY_PROFILE_TRACE": "1"})
        compiler = json.loads(raw)
        require(compiler["compiler"] == COMMIT and compiler["crateName"] == crate, "wrong compiler inventory")
        packages = cargo_packages(root, triple, package, no_default, features, env)
        compiler_args = invocation(compiler_log, rustc, crate)
        compiled = profile(compiler_args, root)
        compiled["externBindings"] = extern_bindings(compiler_log, rustc, compiler_args, packages)
        # Enable unstable JSON output only in rustdoc; dependency compilers
        # and build scripts use the regular stable toolchain environment.
        wrapper = build / "doc-wrapper"
        wrapper.write_text('#!/bin/sh\nexport RUSTC_BOOTSTRAP="$ODD_DOC_CRATE"\nexec "$ODD_RUSTDOC" "$@"\n')
        wrapper.chmod(0o755)
        remap = f"--remap-path-prefix={root}=/oddlang-workspace"
        documentation = env | {"RUSTC": str(rustc), "RUSTDOC": str(wrapper), "ODD_RUSTDOC": str(rustdoc),
                               "ODD_DOC_CRATE": crate, "CARGO_ENCODED_RUSTFLAGS": remap,
                               "CARGO_ENCODED_RUSTDOCFLAGS": remap}
        flags = [tool("cargo"), "rustdoc", "--locked", "--offline", "--release", "--target", triple,
                 "--manifest-path", manifest, "--target-dir", build / "cargo", "-p", package, "-vv"]
        flags.extend(["--lib"] if kind == "lib" else ["--bin", target])
        if no_default:
            flags.append("--no-default-features")
        for feature in features:
            flags.extend(["--features", feature])
        flags.extend(["--", "-Z", "unstable-options", "--output-format", "json", "--document-private-items"])
        for setting in compiled["codegen"]:
            flags.extend(["-C", setting])
        _, doc_log = command(flags, root, documentation)
        doc_args = invocation(doc_log, wrapper, crate)
        observed = profile(doc_args, root)
        observed["externBindings"] = extern_bindings(doc_log, rustc, doc_args, packages)
        compare_profiles(compiled, observed)
        require(compiled["target"] == [triple] and compiled["pathMaps"] ==
                ["--remap-path-prefix=$WORKSPACE=/oddlang-workspace"], "requested target or path map differs")
        document = json.loads((build / "cargo" / triple / "doc" / (crate + ".json")).read_bytes())
        require(document["format_version"] == 57 and document["includes_private"], "unsupported rustdoc schema/scope")
        require(document["target"]["triple"] == triple, "wrong documented target")
        cfg_args = [rustc, "--print", "cfg", "--target", triple]
        for setting in compiled["codegen"]:
            cfg_args.extend(["-C", setting])
        for setting in compiled["explicitCfg"]:
            cfg_args.extend(["--cfg", setting])
        selected_cfg = command(cfg_args, root, env)[0].decode().splitlines()
        result = compare(compiler, document, root)
        legacy = span_evidence(compiler, document, root, previous)
    require(pinned_source(root, revision) == source, "source changed during observation")
    return {"schema": 1, "source": source, "package": package, "targetKind": kind, "defaultFeatures": not no_default,
            "requestedFeatures": sorted(features), "tools": tools, "cargoProfile": "release",
            "profile": compiled, "compilerCfg": sorted(selected_cfg), "rustdocImplicitCfg": ["doc"],
            "cargoLockSha256": trace.sha(root / "Cargo.lock"), "workspaceManifestSha256": trace.sha(manifest),
            "previousSpanReportSha256": trace.sha(previous) if previous else None, "spanEvidence": legacy,
            "formatVersion": 57, "comparison": result}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("workspace", type=Path)
    cli.add_argument("package")
    cli.add_argument("kind", choices=["lib", "bin"])
    cli.add_argument("target")
    cli.add_argument("triple")
    cli.add_argument("--no-default-features", action="store_true")
    cli.add_argument("--features", nargs="+", default=[])
    cli.add_argument("--revision", help="require this exact Git revision and a clean tree")
    cli.add_argument("--previous-spans", type=Path, help="optional recorded legacy span reconciliation")
    cli.add_argument("--out", required=True, type=Path)
    args = cli.parse_args()
    report = collect(args.workspace, args.package, args.kind, args.target, args.triple,
                     args.no_default_features, args.features, args.previous_spans, args.revision)
    args.out.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print({group: len(report["comparison"][group]) for group in ("matched","compilerOnly","rustdocOnly","ambiguous")})


if __name__ == "__main__":
    main()
