"""Inventory pinned upstream source and record the limits of lexical observations."""

import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tempfile
import tomllib

from inventory_platforms import (
    denominators,
    package_inputs,
    test_inventory as platform_tests,
    workflow_test_steps,
)

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1] / "examples/marmish/project.toml"


def run(root, *args, data=None):
    return subprocess.run(
        args, cwd=root, input=data, stdout=subprocess.PIPE, check=True
    ).stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encode(obj):
    return (
        json.dumps(obj, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
        + "\n"
    ).encode()


def classify_file(path):
    p = PurePosixPath(path)
    top, name = p.parts[0], p.name
    if path == "scripts/install.sh" or (
        p.parent.as_posix() == "docs/ontology" and p.suffix in (".jsonl", ".yaml")
    ):
        return "generated-artifact"
    if top == "tests" and "fixtures" in p.parts:
        return "test-fixture"
    if name in (
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "build.rs",
        "uv.lock",
        "pyproject.toml",
        "package.json",
        "package-lock.json",
    ) or top in (".github", "patches"):
        return "build-input"
    if top == "tests" or (top == "crates" and "tests" in p.parts):
        return "test"
    if top == "crates" and p.suffix == ".rs":
        return "cargo-source"
    if top in ("scripts", "tools"):
        if (
            name.startswith(("test_", "test-"))
            or ".test." in name
            or name.endswith("-test.c")
            or name == "test.sh"
        ):
            return "test"
        return "tooling"
    if top in ("docs", ".beads") or path in (
        "AGENTS.md",
        "LICENSE",
        "README.md",
        ".gitattributes",
        ".gitignore",
    ):
        return "static-asset"
    raise ValueError("unclassified Git path: " + path)


def reference_scopes(path, refs, ids):
    result = []
    for ordinal, kind, spelling, line in refs:
        if (
            path not in ids
            or not 0 <= ordinal <= len(ids[path])
            or line < 1
            or not kind
            or not spelling
        ):
            raise ValueError(
                f"invalid lexical reference: {path}:{line} scope {ordinal}"
            )
        result.append(ids[path][ordinal - 1] if ordinal else None)
    return result


def require_complete(gaps):
    if gaps:
        raise ValueError("open completeness obligations: " + "; ".join(gaps))


def files_at_pin(root, pin):
    if run(root, "git", "rev-parse", "HEAD").decode().strip() != pin:
        raise ValueError("upstream revision differs from project manifest")
    if run(root, "git", "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("expected pristine pinned source checkout")
    result = {}
    for entry in run(root, "git", "ls-tree", "-rz", pin).split(b"\0")[:-1]:
        meta, raw = entry.split(b"\t", 1)
        mode, kind, oid = meta.decode().split()
        path = raw.decode()
        if kind != "blob" or mode not in ("100644", "100755"):
            raise ValueError("unsupported Git entry " + path)
        data = (root / path).read_bytes()
        if hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest() != oid:
            raise ValueError("checkout differs from pinned blob: " + path)
        result[path] = {
            "mode": mode,
            "gitBlob": oid,
            "sha256": digest(data),
            "class": classify_file(path),
        }
    return result


def cargo_inventory(root, project):
    args = ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"]
    meta = json.loads(run(root, *args))
    packages = {p["id"]: p for p in meta["packages"]}
    members = {i: packages[i]["name"] for i in meta["workspace_members"]}
    if sorted(members.values()) != sorted(project["workspace_members"]):
        raise ValueError("Cargo workspace differs from pinned manifest")

    def package_id(raw):
        return "workspace:" + members[raw] if raw in members else raw

    crates, targets = {}, {}
    for i, name in members.items():
        p = packages[i]
        deps = [
            {
                k: d[k]
                for k in (
                    "name",
                    "rename",
                    "kind",
                    "req",
                    "target",
                    "features",
                    "uses_default_features",
                    "optional",
                )
            }
            for d in p["dependencies"]
        ]
        crates[name] = {
            "version": p["version"],
            "edition": p["edition"],
            "features": p["features"],
            "dependencyDeclarations": deps,
        }
        for t in p["targets"]:
            tid = name + ":" + t["kind"][0] + ":" + t["name"]
            targets[tid] = {
                "crate": name,
                "kind": t["kind"][0],
                "name": t["name"],
                "path": Path(t["src_path"]).relative_to(root).as_posix(),
                "requiredFeatures": t.get("required-features", []),
                "edition": t["edition"],
            }
    # Resolve dependency aliases, enabled features and conditional edges for each
    # platform/profile. Intern repeated node records to keep the archive small.
    pool, profiles = {}, {}
    for platform in project["targets"]:
        profiles[platform] = {}
        for feature in project["feature_profiles"]:
            extra = [] if feature == "default" else ["--features", feature]
            cfg = json.loads(run(root, *args, "--filter-platform", platform, *extra))
            hashes = []
            for node in cfg["resolve"]["nodes"]:
                row = {
                    "id": package_id(node["id"]),
                    "features": sorted(node["features"]),
                    "deps": sorted(
                        (
                            [d["name"], package_id(d["pkg"]), d["dep_kinds"]]
                            for d in node["deps"]
                        ),
                        key=encode,
                    ),
                }
                key = digest(encode(row))
                pool[key] = row
                hashes.append(key)
            profiles[platform][feature] = sorted(hashes)
    external = {
        i: {
            "name": p["name"],
            "version": p["version"],
            "source": p["source"],
            "class": "external-library",
        }
        for i, p in packages.items()
        if i not in members
    }
    return {
        "crates": crates,
        "targets": targets,
        "external": external,
        "resolvedNodes": pool,
        "profiles": profiles,
    }, members


def attrs_at(syntax, path, line):
    return [
        attr
        for start, end, attrs in syntax.get(path, {}).get("attributes", [])
        if start <= line <= end
        for attr in attrs
    ]


def module_contexts(files, targets, syntax):
    """Retain candidate Cargo roots and cfg on literal source-module edges."""
    contexts, missing = defaultdict(list), set()
    for tid, t in targets.items():
        pending = [
            (
                t["path"],
                True,
                tuple(f'#[cfg(feature = "{f}")]' for f in t["requiredFeatures"]),
            )
        ]
        seen = set()
        while pending:
            path, is_root, cfg = pending.pop()
            if (path, cfg) in seen:
                continue
            seen.add((path, cfg))
            if path not in files:
                raise ValueError("missing target input: " + path)
            contexts[path].append({"target": tid, "attributes": cfg})
            p = PurePosixPath(path)
            for edge in syntax.get(path, {}).get("edges", []):
                if edge["kind"] == "include":
                    candidates = [p.parent / edge["path"]] if edge["path"] else []
                else:
                    if edge["path"]:
                        candidates = [p.parent.joinpath(*edge["inline"], edge["path"])]
                    else:
                        base = (
                            p.parent
                            if is_root or p.name == "mod.rs"
                            else p.with_suffix("")
                        )
                        child = base.joinpath(*edge["inline"], edge["name"])
                        candidates = [child.with_suffix(".rs"), child / "mod.rs"]
                matches = [
                    os.path.normpath(str(c))
                    for c in candidates
                    if os.path.normpath(str(c)) in files
                ]
                if len(matches) != 1:
                    missing.add((path, edge["line"], tid))
                    continue
                attrs = tuple(
                    sorted(set(cfg) | set(attrs_at(syntax, path, edge["line"])))
                )
                pending.append((matches[0], False, attrs))
    return contexts, sorted(missing)


def test_only(attrs):
    for attr in attrs:
        text = "".join(attr.split())
        if text in ("#[test]", "#[cfg(test)]", "#[tokio::test]") or text.startswith(
            "#[tokio::test("
        ):
            return True
        if text.startswith("#[cfg(all(") and "test" in text[10:-3].split(","):
            return True
    return False


def classify_symbol(path, file_class, contexts, attrs, targets):
    if file_class not in ("cargo-source", "test") or not path.startswith("crates/"):
        return file_class, "tracked " + file_class
    if not contexts:
        return "unsupported", "Rust declaration without resolved Cargo module root"
    if (
        file_class == "test"
        or test_only(attrs)
        or all(
            test_only(c["attributes"]) or targets[c["target"]]["kind"] == "test"
            for c in contexts
        )
    ):
        return "test", "test target or explicit test-only declaration/module"
    if all(targets[c["target"]]["kind"] == "example" for c in contexts):
        return "example", "Cargo example target"
    if all(
        any('feature = "installed-checks"' in a for a in c["attributes"])
        for c in contexts
    ):
        return "qualification-tool", "Cargo installed-checks feature"
    if all(targets[c["target"]]["name"] == "marmish-release-tool" for c in contexts):
        return "build-input", "release packaging target"
    return "retained-native", "original native definition in Cargo source closure"


def checked_framing(root, members, project):
    compiler = run(root, "rustc", "-Vv").decode().strip()
    if "release: " + project["rust"] + "\n" not in compiler:
        raise ValueError("Rust compiler differs from project")
    # Check a consumer using the produced rustc metadata. Compiling these constant
    # assignments checks public paths, signatures, error identity and bound types.
    probe = """use marmish_core::{FrameError,Limits,ProbeEnvelope};
type Codec = fn(&[u8]) -> Result<Vec<u8>, FrameError>;
const _: Codec = ProbeEnvelope::encode;
const _: Codec = ProbeEnvelope::decode;
const _: [usize;3] = [Limits::RAW_BYTES,Limits::JSON_BYTES,Limits::NDJSON_BYTES];
const _: [FrameError;4] = [FrameError::SizeLimit,FrameError::InvalidJson,
                           FrameError::InvalidVersion,FrameError::InvalidEncoding];
"""
    with tempfile.TemporaryDirectory() as d:
        temp = Path(d)
        raw = run(
            root,
            "cargo",
            "check",
            "--locked",
            "--offline",
            "--lib",
            "-p",
            "marmish-core",
            "--target-dir",
            d,
            "--message-format=json",
        )
        artifacts = [
            a
            for line in raw.splitlines()
            if (a := json.loads(line))["reason"] == "compiler-artifact"
        ]
        produced = next(
            a
            for a in artifacts
            if a["package_id"] in members
            and members[a["package_id"]] == "marmish-core"
            and a["target"]["kind"] == ["lib"]
        )
        rmeta = next(Path(p) for p in produced["filenames"] if p.endswith(".rmeta"))
        run(
            root,
            "rustc",
            "--edition=2021",
            "--crate-type=lib",
            "--emit=metadata",
            "--crate-name",
            "frame_consumer",
            "-L",
            "dependency=" + str(rmeta.parent),
            "--extern",
            "marmish_core=" + str(rmeta),
            "-o",
            str(temp / "consumer.rmeta"),
            "-",
            data=probe.encode(),
        )
        compiled = sorted(
            set(
                (
                    "workspace:" + members[a["package_id"]]
                    if a["package_id"] in members
                    else a["package_id"]
                )
                for a in artifacts
            )
        )
    return {
        "compiler": compiler,
        "source": project["framing_source"],
        "consumer": probe,
        "compiledPackages": compiled,
        "status": "type-checked",
        "scope": "public framing methods, named errors and limit types",
    }


def generate(root):
    project = tomllib.loads(PROJECT.read_text())
    files = files_at_pin(root, project["git_revision"])

    def ontology(name):
        return [
            json.loads(line)
            for line in (root / "docs/ontology" / f"{name}.jsonl")
            .read_bytes()
            .splitlines()
        ]

    source_files, symbols, references = map(
        ontology, ["source_files", "symbols", "code_references"]
    )
    for row in source_files:
        f = files[row["path"]]
        if (
            f["sha256"] != row["sha256"]
            or f["mode"] != row["mode"]
            or "parseStatus" in f
        ):
            raise ValueError("ontology file mismatches Git tree: " + row["path"])
        f.update(language=row["language"], parseStatus=row["parse_status"])
    cargo, members = cargo_inventory(root, project)
    targets = cargo["targets"]
    syntax_data = json.loads(
        run(
            root,
            "uv",
            "run",
            "--offline",
            "--locked",
            "--python",
            "3.12",
            "--project",
            "tools/code-ontology",
            "python",
            str(HERE / "inventory_syntax.py"),
            str(root),
            data=encode(
                [p for p in files if p.startswith("crates/") and p.endswith(".rs")]
            ),
        )
    )
    syntax, workflows = syntax_data["rust"], syntax_data["workflows"]
    contexts, missing = module_contexts(files, targets, syntax)
    for path, items in contexts.items():
        files[path]["cargoContexts"] = sorted(items, key=encode)
    ids, all_ids = defaultdict(list), set()
    for s in symbols:
        path = s["path"]
        id_ = s["id"]
        if id_ in all_ids or not 1 <= s["start_line"] <= s["end_line"]:
            raise ValueError("invalid symbol: " + id_)
        all_ids.add(id_)
        ids[path].append(id_)
        attrs = attrs_at(syntax, path, s["start_line"])
        klass, reason = classify_symbol(
            path, files[path]["class"], contexts.get(path, []), attrs, targets
        )
        s.update(
            classification=klass,
            reason=reason,
            attributes=attrs,
            targets=sorted({c["target"] for c in contexts.get(path, [])}),
        )
    if any(s["parent_id"] and s["parent_id"] not in ids[s["path"]] for s in symbols):
        raise ValueError("missing lexical symbol parent")
    ref_count = 0
    for r in references:
        if r["path"] not in files:
            raise ValueError("reference to missing file")
        ids.setdefault(r["path"], [])
        scopes = reference_scopes(r["path"], r["references"], ids)
        ref_count += len(scopes)
    classes = Counter(s["classification"] for s in symbols)
    parser = Counter(r["parse_status"] for r in source_files)
    gaps = [
        "Lexical callees, generated declarations and workspace-wide symbol reachability remain unresolved.",
        "Source parser recovery and omission markers preclude a complete declaration denominator.",
    ]
    if missing:
        gaps.append(f"{len(missing)} unresolved target/module edges.")
    if classes["unsupported"]:
        gaps.append(f"{classes['unsupported']} unsupported lexical declarations.")
    framing = checked_framing(root, members, project)
    cfg = {
        t: sorted(
            run(root, "rustc", "--print", "cfg", "--target", t).decode().splitlines()
        )
        for t in project["targets"]
    }
    denominators_report = denominators(
        symbols, files, project["targets"], project["feature_profiles"]
    )
    tests, by_platform = platform_tests(
        syntax=syntax,
        files=files,
        symbols=symbols,
        targets=targets,
        cfgs=cfg,
        profiles=project["feature_profiles"],
    )
    packages = {t: package_inputs(t, files, workflows, targets) for t in cfg}
    test_sets = {t: {r["test"] for r in rows} for t, rows in by_platform.items()}
    common_tests = set.intersection(*test_sets.values())
    platform_summary = {
        t: {
            "observedTestSites": len(test_sets[t]),
            "platformRestrictedSites": sorted(test_sets[t] - common_tests),
            "ignoredSites": sorted({r["test"] for r in rows if r["gate"] == "ignored"}),
            "unknownConfigurationSites": sorted(
                {r["test"] for r in rows if r["uncertainProfiles"]}
            ),
            "packaging": packages[t],
        }
        for t, rows in by_platform.items()
    }
    archive = {
        "schemaVersion": 2,
        "gitRevision": project["git_revision"],
        "manifestSha256": digest(PROJECT.read_bytes()),
        "scope": project["inventory"],
        "denominators": denominators_report,
        "testSites": tests,
        "testApplicability": by_platform,
        "workflowTestSteps": workflow_test_steps(workflows),
        "platformPackaging": packages,
        "files": files,
        "cargo": cargo,
        "rustcCfg": cfg,
        "symbols": symbols,
        "moduleSyntax": syntax,
        "unresolvedModuleEdges": missing,
        "references": references,
        "referenceScope": "ordinal zero = file; positive = one-based symbol in same path, preserving symbols array order",
        "framingCompilerCheck": framing,
        "gaps": gaps,
    }
    counts = {
        "files": len(files),
        "fileClasses": dict(Counter(f["class"] for f in files.values())),
        "ontologyFiles": len(source_files),
        "parseStatus": dict(parser),
        "symbols": len(symbols),
        "symbolClasses": dict(classes),
        "lexicalReferences": ref_count,
        "cargoTargets": len(targets),
        "workspaceMembers": len(members),
        "externalPackages": len(cargo["external"]),
        "runtimeLexicalSitesByDisposition": {
            k: classes[k] for k in ("imported", "retained-native", "unsupported")
        },
    }
    return archive, {
        "schemaVersion": 2,
        "gitRevision": project["git_revision"],
        "counts": counts,
        "denominators": denominators_report,
        "platformInventory": platform_summary,
        "sourceInventorySha256": digest(
            encode({k: v for k, v in archive.items() if k != "framingCompilerCheck"})
        ),
        "completeness": "open",
        "gaps": gaps,
        "framingCompilerCheck": framing["status"],
        "host": next(
            line.removeprefix("host: ")
            for line in framing["compiler"].splitlines()
            if line.startswith("host: ")
        ),
        "unresolvedModuleEdges": len(missing),
        "manifestSha256": digest(PROJECT.read_bytes()),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--require-complete", action="store_true")
    args = parser.parse_args()
    archive, summary = generate(args.upstream.resolve())
    compressed = io.BytesIO()
    with gzip.GzipFile(
        fileobj=compressed, mode="wb", filename="", mtime=0, compresslevel=6
    ) as zipped:
        zipped.write(encode(archive))
    data = compressed.getvalue()
    summary["inventorySha256"] = digest(data)
    args.output.mkdir(exist_ok=True, parents=True)
    (args.output / "inventory.json.gz").write_bytes(data)
    (args.output / "inventory-summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n"
    )
    if args.require_complete:
        require_complete(summary["gaps"])


if __name__ == "__main__":
    main()
