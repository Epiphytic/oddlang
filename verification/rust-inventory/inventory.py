#!/usr/bin/env python3
"""Collect a nightly Rust library definition inventory and classify dep-info coverage."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shlex
import subprocess
import sys
import tomllib

TOOLCHAIN = Path(__file__).with_name("rust-toolchain.toml")
COMPILER_COMMIT = "8925ea358a0f265ca61026aadc7ecc506c545cbe"
RUSTDOC_FORMAT = 61


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    with path.open("rb") as data:
        return hashlib.file_digest(data, "sha256").hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")


def run(args, root, env, output, name, sandbox=None):
    prefix = ["/usr/bin/sandbox-exec", "-f", sandbox] if sandbox else []
    proc = subprocess.run([*prefix, *args], cwd=root, env=env, stdin=subprocess.DEVNULL, capture_output=True)
    (output / f"{name}.stdout").write_bytes(proc.stdout)
    (output / f"{name}.stderr").write_bytes(proc.stderr)
    require(proc.returncode == 0, f"{name} failed; see {output}/{name}.stderr")
    return proc.stdout.decode()


def flag(words, name):
    return [words[i + 1] for i, word in enumerate(words[:-1]) if word == name]


def commands(log):
    pending = None
    for line in log.splitlines():
        start = re.match(r"\s+Running `(.*)", line)
        if pending is None and start:
            pending = start[1]
        elif pending is not None:
            pending += "\n" + line
        if pending is not None and pending.endswith("`"):
            try:
                words = shlex.split(pending[:-1])
            except ValueError:
                continue
            yield words
            pending = None
    require(pending is None, "incomplete verbose Cargo command")


def dep_inputs(depfile):
    content = depfile.read_text().replace("\\\n", "")
    first_rule = content.splitlines()[0]
    _, inputs = first_rule.split(": ", 1)
    return [re.sub(r"\\(.)", r"\1", word) for word in re.findall(r"(?:\\.|[^\s\\])+", inputs)]


def package_id(identity, root):
    prefix = f"path+{root.as_uri()}"
    if identity.startswith(prefix + "/") or identity.startswith(prefix + "#"):
        return "workspace:" + identity.removeprefix(prefix).removeprefix("/")
    return identity


def input_identity(path, roots, producers):
    for scope, root in roots:
        if not path.is_relative_to(root):
            continue
        relative = path.relative_to(root).as_posix()
        if scope == "registry-cache":
            return ["registry", relative.split("/", 1)[1]]
        if scope == "target":
            for producer, package in producers.items():
                if path.is_relative_to(producer):
                    return ["build", package + "/" + path.relative_to(producer).as_posix()]
            raise ValueError(f"no recorded producer for generated input {path}")
        return [scope, relative]
    return None


def collect_invocations(root, target, toolchain, roots, producers, log):
    compiler = str(toolchain / "bin/rustc")
    result = []
    paired = set()

    def identify(raw):
        path = (root / raw).resolve(strict=True)
        identity = input_identity(path, roots, producers)
        require(identity is not None, f"undeclared nightly compiler input {path}")
        return [*identity, sha(path)]

    for invocation in commands(log):
        if compiler not in invocation:
            continue
        index = invocation.index(compiler)
        env = dict(word.split("=", 1) for word in invocation[:index] if "=" in word)
        args = invocation[index + 1:]
        crate = flag(args, "--crate-name")[0]
        out = Path(flag(args, "--out-dir")[0])
        suffix = next((value.removeprefix("extra-filename=") for value in flag(args, "-C")
                       if value.startswith("extra-filename=")), "")
        depfile = out / (crate + suffix + ".d")
        require(depfile not in paired, f"reused compiler dep-info {depfile}")
        paired.add(depfile)
        paths = dep_inputs(depfile)
        source = next((arg for arg in args if arg in paths), None)
        require(source is not None, f"compiler crate root missing in {depfile}")
        result.append({
            "crate": crate,
            "package": [env["CARGO_PKG_NAME"], env["CARGO_PKG_VERSION"]],
            "root": identify(source),
            "inputs": sorted(identify(path) for path in paths),
            "test": "--test" in args,
            "explicitCfg": sorted(flag(args, "--cfg")),
            "externAliases": sorted({value.split("=", 1)[0] for value in flag(args, "--extern")}),
        })
    require(result, "no nightly Rust invocations recorded")
    require(paired == set(target.rglob("*.d")),
            "nightly Cargo emitted unpaired dep-info")
    return sorted(result, key=lambda row: json.dumps(row, sort_keys=True))


def definition_inventory(document, roots, producers, workspace):
    require(document["format_version"] == RUSTDOC_FORMAT, "nightly rustdoc JSON version changed")
    paths = document["paths"]
    local = [item for item in document["index"].values() if item["crate_id"] == 0]
    local_ids = {item["id"] for item in local}

    def types(tree):
        refs = set()
        if isinstance(tree, dict):
            path = tree.get("resolved_path")
            if isinstance(path, dict):
                identity = path["id"]
                refs.add((identity, path["path"]))
            for value in tree.values():
                refs.update(types(value))
        elif isinstance(tree, list):
            for value in tree:
                refs.update(types(value))
        return refs

    result = []
    for item in local:
        definition_kind, payload = next(iter(item["inner"].items()))
        identity = item["id"]
        resolved = []
        for reference, spelling in sorted(types(payload)):
            documented = paths.get(str(reference))
            resolved.append({
                "spelling": spelling,
                "rustdocId": reference,
                "qualifiedPath": documented["path"] if documented else None,
                "status": ("qualified-path" if documented else "local-id"
                           if reference in local_ids else "opaque-rustdoc-id"),
            })
        span = item["span"]
        if span is not None:
            path = Path(span["filename"])
            path = workspace / path.relative_to("/oddlang-workspace") if path.is_relative_to("/oddlang-workspace") else path
            file_id = input_identity((workspace / path).resolve(), roots, producers)
            span = {"file": file_id or ["external-span", span["filename"]],
                    "begin": span["begin"], "end": span["end"]}
        result.append({
            "rustdocId": identity,
            "name": item["name"],
            "kind": definition_kind,
            "span": span,
            "path": paths.get(str(identity), {}).get("path"),
            "signatureTypeReferences": resolved,
            "macroExpansionOrigin": "unavailable",
        })
    return sorted(result, key=lambda row: row["rustdocId"])


def compare_stable(report, stable):
    stable_rows = stable["builds"]["check"]["rustcInputs"]
    stable_set = {tuple(item) for row in stable_rows for item in row["inputs"]}
    nightly_set = {tuple(item) for row in report["compilerInvocations"] for item in row["inputs"]}
    stable_scope = {item for item in stable_set if item[0] == "workspace"}
    nightly_scope = {item for item in nightly_set if item[0] == "workspace"}
    require(stable_scope and stable_scope == nightly_scope, "stable and nightly pinned package source identities differ")
    compared = {
        "stableScopeEntries": len(stable_scope),
        "entireInputClosureMatches": stable_set == nightly_set,
        "stableAndNightlyPinnedSourceIdentitiesMatch": True,
        "onlyStableInputs": sorted(stable_set - nightly_set),
        "onlyNightlyInputs": sorted(nightly_set - stable_set),
    }
    return compared


def bind_bundle(bundle, pin, package_root):
    require(bundle is not None and pin is not None, "bundle verification requires --bundle and --bundle-sha256")
    lock = bundle / "bundle.lock"
    require(sha(lock) == pin, "unexpected syntax bundle lock digest")
    layer = json.loads(lock.read_text())
    inventory = bundle / "sources.json"
    require(layer["version"] == 1 and sha(bundle / "syntax.ttl") == layer["syntaxSha256"]
            and sha(inventory) == layer["sourcesSha256"], "syntax bundle content does not match lock")
    sources = json.loads(inventory.read_text())["files"]
    require(sources, "syntax bundle has an empty source inventory")
    for name, expected in sources.items():
        relative = PurePosixPath(name)
        path = package_root / relative
        require(not relative.is_absolute() and ".." not in relative.parts and name == relative.as_posix()
                and path.resolve() == path and path.is_file(), f"invalid syntax bundle source {name}")
        require(sha(path) == expected, f"stale syntax bundle source digest: {name}")
    return {"lockSha256": pin, "syntaxSha256": layer["syntaxSha256"],
            "sourcesSha256": layer["sourcesSha256"], "sourcesMatched": len(sources)}


def collect(args):
    root = args.workspace.resolve(strict=True)
    output = args.output.resolve()
    cargo_home = args.cargo_home.resolve(strict=True)
    vendor = args.vendor.resolve(strict=True) if args.vendor else None
    sandbox = args.sandbox.resolve(strict=True) if args.sandbox else None
    require(not output.is_relative_to(root), "place the inventory output outside the workspace")
    output.mkdir()
    target = output / "target"
    manifest = root / "Cargo.toml"
    channel = tomllib.loads(TOOLCHAIN.read_text())["toolchain"]["channel"]
    cargo = Path(subprocess.check_output(
        ["rustup", "which", "--toolchain", channel, "cargo"], text=True
    ).strip()).resolve()
    toolchain = cargo.parents[1]
    rustc = toolchain / "bin/rustc"
    rustdoc = toolchain / "bin/rustdoc"
    env = {
        "HOME": str(output / "home"),
        "CARGO_HOME": str(cargo_home),
        "CARGO_TARGET_DIR": str(target),
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never",
        "RUSTC": str(rustc),
        "RUSTDOC": str(rustdoc),
        "PATH": f"{toolchain}/bin:/usr/bin:/bin",
        "CARGO_ENCODED_RUSTFLAGS": f"--remap-path-prefix={root}=/oddlang-workspace",
        "CARGO_ENCODED_RUSTDOCFLAGS": f"--remap-path-prefix={root}=/oddlang-workspace",
        "TMPDIR": str(output / "tmp"),
        "LC_ALL": "C",
        "TZ": "UTC",
    }
    sdk = None
    if sys.platform == "darwin":
        sdk = Path(subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()).resolve()
        clang = Path(subprocess.check_output(["xcrun", "--find", "clang"], text=True).strip()).resolve()
        env.update({"SDKROOT": str(sdk), "CC": str(clang), "CXX": str(clang.parent / "clang++")})
        host = subprocess.check_output([rustc, "-vV"], text=True).split("host: ")[1].splitlines()[0]
        host_key = host.upper().replace("-", "_")
        env[f"CARGO_TARGET_{host_key}_LINKER"] = str(clang)
    Path(env["HOME"]).mkdir()
    Path(env["TMPDIR"]).mkdir()
    compiler = run([rustc, "-vV"], root, env, output, "rustc")
    require(f"commit-hash: {COMPILER_COMMIT}" in compiler, "wrong pinned nightly Rust compiler")
    cfg = run([rustc, "--print", "cfg"], root, env, output, "cfg")
    flags = ["--locked", "--offline", "--manifest-path", str(manifest), "-p", args.package]
    if args.features:
        flags.extend(["--features", args.features])
    if args.no_default_features:
        flags.append("--no-default-features")
    metadata = [cargo, "metadata", "--locked", "--offline", "--no-deps", "--format-version", "1",
                "--manifest-path", str(manifest)]
    package = next((row for row in json.loads(run(metadata, root, env, output, "metadata", sandbox))["packages"]
                    if row["name"] == args.package), None)
    require(package is not None, "package is absent from workspace metadata")
    library = next((target for target in package["targets"]
                    if target["doc"] and set(target["kind"]) & {"lib", "rlib", "dylib", "proc-macro", "cdylib", "staticlib"}), None)
    require(library is not None, "expected a documentable package library")
    check = [cargo, "check", *flags, "--all-targets", "--message-format=json", "-vv"]
    log = run(check, root, env, output, "check", sandbox)
    messages = [json.loads(line) for line in log.splitlines() if line.startswith('{"reason":')]
    artifact = next((row for row in messages if row["reason"] == "compiler-artifact"
                     and row["package_id"] == package["id"] and row["target"] == library
                     and not row["profile"]["test"]), None)
    require(artifact is not None, "expected a documentable package library")
    doc = [cargo, "rustdoc", *flags, "--lib", "--", "-Z", "unstable-options",
           "--output-format", "json", "--document-private-items"]
    run(doc, root, env, output, "rustdoc", sandbox)
    doc_file = target / "doc" / (artifact["target"]["name"] + ".json")
    producers = {Path(row["out_dir"]): package_id(row["package_id"], root)
                 for row in messages if row["reason"] == "build-script-executed"}
    roots = [("workspace", root), ("target", target), ("registry-cache", cargo_home / "registry/src"),
             ("git-dependency", cargo_home / "git/checkouts"), ("toolchain", toolchain)]
    if vendor is not None:
        roots.insert(0, ("registry", vendor))
    if sdk is not None:
        roots.append(("sdk", sdk))
    definitions = definition_inventory(json.loads(doc_file.read_text()), roots, producers, root)
    inputs = collect_invocations(root, target, toolchain, roots, producers, (output / "check.stderr").read_text())
    manifest_path = Path(artifact["manifest_path"]).relative_to(root).as_posix()
    report = {
        "version": 1,
        "nightly": channel,
        "compiler": compiler.strip(),
        "cargo": run([cargo, "-V"], root, env, output, "cargo").strip(),
        "rustdocFormat": RUSTDOC_FORMAT,
        "crate": package_id(artifact["package_id"], root),
        "configuration": {
            "features": artifact["features"], "targetCfg": sorted(cfg.splitlines()),
            "definitionScope": "nightly rustdoc library with private items",
            "inputScope": "nightly Cargo check all targets with selected features",
        },
        "coverage": {
            "definitions": "rustdoc-emitted inventory only",
            "signatureTypes": "resolved Rustdoc references with explicit qualified path or IDs",
            "callResolution": "unavailable in rustdoc JSON",
            "bodyTypes": "unavailable in rustdoc JSON",
            "macroExpansionOrigin": "unavailable in rustdoc JSON",
            "inactiveBranches": "outside the selected configuration; unresolved",
        },
        "manifests": {path: sha(root / path) for path in sorted({"Cargo.toml", "Cargo.lock", manifest_path})},
        "commands": [[str(arg) for arg in command] for command in (check, doc)],
        "compilerInvocations": inputs,
        "definitionCounts": dict(Counter(row["kind"] for row in definitions)),
        "definitions": definitions,
        "rawRustdocSha256": sha(doc_file),
        "workspaceRoot": str(root),
    }
    if args.bundle or args.bundle_sha256:
        report["syntaxBundle"] = bind_bundle(args.bundle, args.bundle_sha256, (root / manifest_path).parent)
    if args.stable_receipt:
        stable_doc = json.loads(args.stable_receipt.read_text())
        require(stable_doc["version"] == 2, "expected stable stream baseline receipt version 2")
        for path, digest in report["manifests"].items():
            require(stable_doc["cargoInputs"].get(path) == digest, f"stable manifest or lock changed: {path}")
        report["stableComparison"] = compare_stable(report, stable_doc)
        report["stableComparison"]["receiptSha256"] = sha(args.stable_receipt)
    if args.matching_report:
        previous = json.loads(args.matching_report.read_text())
        require(previous["workspaceRoot"] != report["workspaceRoot"], "candidate and original workspace roots match")
        for field in ("compiler", "cargo", "crate", "configuration", "manifests", "compilerInvocations",
                      "definitionCounts", "definitions"):
            require(previous[field] == report[field], f"nightly candidate differs: {field}")
        report["matchingReportSha256"] = sha(args.matching_report)
    save(output / "rust-inventory.json", report)
    print(output / "rust-inventory.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", type=Path)
    parser.add_argument("package")
    parser.add_argument("output", type=Path)
    parser.add_argument("--stable-receipt", type=Path)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--bundle-sha256")
    parser.add_argument("--matching-report", type=Path)
    parser.add_argument("--vendor", type=Path, help="versioned vendor directory configured in --cargo-home")
    parser.add_argument("--sandbox", type=Path, help="apply a Seatbelt profile to Cargo subprocesses")
    parser.add_argument("--features")
    parser.add_argument("--no-default-features", action="store_true")
    parser.add_argument("--cargo-home", type=Path, default=Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")))
    try:
        collect(parser.parse_args())
    except (OSError, subprocess.CalledProcessError, ValueError) as e:
        print(e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
