"""Qualify fresh native and component observations for the reconstructed workspace."""

import argparse
import hashlib
import re
import tomllib
from concurrent.futures import ThreadPoolExecutor
import gzip
import json
from pathlib import Path
import subprocess
import sys

from gaps import generate
from observations import CONTRACT, expand
from prepare import ROOT, digest
from rebuild import ONTOLOGY, PIN, rebuild
from qualify_candidate import compare_archived
from frame_runner import verify_inputs
from receipt import expected_cases

HERE = Path(__file__).resolve().parent


def coverage(inventory, generated, frozen_register=None):
    crates = inventory["cargo"]["crates"]
    sites = [
        s for s in inventory["symbols"] if s["classification"] == "retained-native"
    ]
    for path in {s["path"] for s in sites if s["path"] not in generated}:
        parts = path.split("/")
        if len(parts) < 3 or parts[0] != "crates" or parts[1] not in crates:
            raise ValueError(f"retained path has no workspace package: {path}")
    gaps = generate(inventory, generated)
    if frozen_register is not None and frozen_register != gaps:
        raise ValueError(
            "published residual register differs from the actual producer coverage"
        )
    retained = list(gaps["modules"])
    # Prioritize dependencies of first-party packages. Calls inside these packages
    # are still lexical; they do not establish a resolved function ordering.
    dependencies = {
        name: {
            dep["name"]
            for dep in crate["dependencyDeclarations"]
            if dep["name"] in crates and dep["kind"] != "dev"
        }
        for name, crate in crates.items()
    }
    order = []
    while dependencies:
        ready = sorted(name for name, deps in dependencies.items() if not deps)
        if not ready:
            raise ValueError("cycle among first-party package dependencies")
        order.extend(ready)
        for name in ready:
            del dependencies[name]
        for deps in dependencies.values():
            deps.difference_update(ready)
    retained.sort(key=lambda m: (order.index(m["path"].split("/")[1]), m["path"]))
    imported = sum(s["path"] in generated for s in sites)
    if sum(m["lexicalSites"] for m in retained) + imported != len(sites):
        raise ValueError(
            "residual register does not balance the original lexical denominator"
        )
    return {
        "unit": "declaration sites in the frozen upstream runtime-source inventory",
        "regeneratedSites": imported,
        "retainedSites": len(sites) - imported,
        "knownSites": len(sites),
        "compiledRuntimeDenominator": None,
        "packageDependencyOrder": order,
        "retainedModules": retained,
        "blockingCapabilities": [
            t for t in gaps["tasks"] if any(m["blockedOn"] == t["id"] for m in retained)
        ],
    }


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"missing or redirected evidence: {path}")
    return path.read_bytes()


def read_json(path):
    return json.loads(read(path))


def check_wire(contract, wire):
    exchanges = [
        {
            "id": c["id"],
            "rawSha256": digest(expand(c["input"])),
            "wireSha256": digest(expand(c["expected"]["bytes"])),
        }
        for c in contract["cases"]
        if c["operation"] == "encode" and "bytes" in c["expected"]
    ]
    require(
        exchanges and len({e["id"] for e in exchanges}) == len(exchanges),
        "invalid contract exchange identities",
    )
    expected = {
        "cases": expected_cases(contract),
        "publicApi": {"original": "tested", "candidate": "tested"},
        "directions": [
            {"writer": writer, "reader": reader, "exchanges": exchanges}
            for writer, reader in [("original", "candidate"), ("candidate", "original")]
        ],
    }
    require(
        wire == expected,
        "mixed wire role, case identity or observed bytes disagree with contract",
    )


def check_platforms(platforms):
    require(
        platforms
        == {
            "aarch64-apple-darwin": "observed",
            "x86_64-unknown-linux-gnu": "unobserved",
            "aarch64-unknown-linux-gnu": "unobserved",
        },
        "platform observation missing or changed",
    )


def check_generated_module(path, expected):
    body = read(path)
    require(
        digest(body) == expected,
        f"generated output differs from producer receipt: {path}",
    )
    return body


def check_source_rows(rows, files, expected_generated):
    require(
        len(rows) == len(files) and {r["path"] for r in rows} == files.keys(),
        "assembly paths differ from original inventory",
    )
    generated = set()
    for r in rows:
        path = r["path"]
        require(r["mode"] == files[path]["mode"], f"assembly mode differs: {path}")
        if r["producer"] == "generated":
            generated.add(path)
        else:
            require(
                r["producer"] == "retained" and r["sha256"] == files[path]["sha256"],
                f"retained source digest differs: {path}",
            )
    require(generated == expected_generated, "unexpected generated module set")


def git_object(kind, data):
    return hashlib.sha1(f"{kind} {len(data)}\0".encode() + data).hexdigest()


def tree_hash(entries):
    tree = {}
    for path, (mode, oid) in entries.items():
        sub = tree
        *parents, name = path.split("/")
        for part in parents:
            sub = sub.setdefault(part, {})
        sub[name] = (mode, oid)

    def hash_directory(node):
        records = []
        for name, value in node.items():
            if isinstance(value, dict):
                mode, oid = "40000", hash_directory(value)
                sort_key = name.encode() + b"/"
            else:
                mode, oid = value
                sort_key = name.encode()
            records.append(
                (
                    sort_key,
                    mode.encode() + b" " + name.encode() + b"\0" + bytes.fromhex(oid),
                )
            )
        return git_object("tree", b"".join(data for _, data in sorted(records)))

    return hash_directory(tree)


PRODUCERS = {"framing": "frame.rs", "facade": "lib.rs"}


def verify_build_pair(dir, inventory):
    reconstruction = read_json(dir / "rebuild.json")
    generated_paths = {f"crates/marmish-core/src/{m}" for m in PRODUCERS.values()}
    pinned_manifest = (
        read(ROOT / "examples/marmish/assembly-v2.sha256").decode().split()[0]
    )
    releases = {
        p: read_json(ROOT / "examples/marmish" / (p + "-release.json"))
        for p in PRODUCERS
    }
    previous = None
    for build in ["first", "second"]:
        base = dir / build
        manifest = read(base / "project.toml")
        require(
            digest(manifest) == pinned_manifest,
            "assembly manifest differs from frozen release",
        )
        rows = tomllib.loads(manifest.decode())["files"]
        check_source_rows(rows, inventory["files"], generated_paths)
        row_by_path = {r["path"]: r for r in rows}
        git_entries = {
            p: (f["mode"], f["gitBlob"]) for p, f in inventory["files"].items()
        }
        generated = {}
        for name, module in PRODUCERS.items():
            release = releases[name]
            imported, projection = base / name, base / (name + "-projection")
            lock_file = read(imported / "import.lock")
            require(
                digest(lock_file) == release["importSha256"],
                f"{build} {name} unpinned input",
            )
            lock = json.loads(lock_file)
            expected_files = {
                f: digest(read(imported / f)) for f in ("source-map.json", "domain.ttl")
            }
            require(
                lock["files"] == expected_files
                and lock["graphSha256"]
                == expected_files["domain.ttl"]
                == release["graphSha256"],
                f"{build} {name} import bytes disagree with lock",
            )
            receipt = read(projection / "projection.json")
            require(
                digest(receipt) == release["projectionSha256"],
                f"{build} {name} unpinned projection",
            )
            published = json.loads(receipt)
            path = "crates/marmish-core/src/" + module
            r = row_by_path[path]
            require(
                published
                == {
                    "version": 1,
                    "graphSha256": release["graphSha256"],
                    "files": {module: r["sha256"]},
                },
                "projection receipt disagrees with assembly",
            )
            blob = git_object(
                "blob", check_generated_module(projection / module, r["sha256"])
            )
            original = git_entries[path][1]
            generated[path] = {
                "originalBlob": original,
                "candidateBlob": blob,
                "sha256": r["sha256"],
            }
            git_entries[path] = (r["mode"], blob)
            if name == "framing":
                require(
                    digest(read(projection / "world.wit")) == release["witSha256"],
                    "framing interface changed",
                )
        result = read_json(base / "result.json")
        require(
            result["assemblySha256"] == pinned_manifest
            and result["generated"] == generated
            and result["sourceFiles"] == len(rows)
            and result["retainedFiles"] == len(rows) - len(generated)
            and result["stagedAssemblyTree"] == tree_hash(git_entries)
            and set(result["regeneratedOntology"]) == ONTOLOGY
            and len(result["regeneratedOntology"]) == len(ONTOLOGY)
            and all(
                re.fullmatch(r"[0-9a-f]{40}", result[k])
                for k in ["candidateCommit", "candidateTree"]
            ),
            f"{build} result disagrees with raw assembly evidence",
        )
        logs = {
            f: read(base / f) for f in ("ontology-update.txt", "ontology-check.txt")
        }
        require(
            logs["ontology-check.txt"].startswith(b"code ontology check: ")
            and logs["ontology-update.txt"].replace(
                b"ontology update:", b"ontology check:", 1
            )
            == logs["ontology-check.txt"],
            "missing ontology check or update observations",
        )
        if previous is not None:
            require(previous == (result, logs), "independent rebuild results differ")
        previous = result, logs
    require(
        reconstruction
        == {"originalCommit": PIN, "independentBuilds": 2, **previous[0]},
        "rebuild summary disagrees with independent runs",
    )
    return reconstruction


def check_frame(directory, contract):
    base = directory / "frame"
    observed = base / "observations"
    frame = read_json(observed / "report.json")
    positive = base / "projection"
    verify_inputs(base / "import", positive, base / "mutant-projection")
    for name in ("domain.ttl", "import.lock", "source-map.json"):
        require(
            read(base / "import" / name)
            == read(directory / "rebuild/first/framing" / name),
            "framing input differs from reconstructed workspace",
        )
    for name in ("frame.rs", "world.wit", "projection.json"):
        require(
            read(positive / name)
            == read(directory / "rebuild/first/framing-projection" / name),
            "framing observation uses a different projection",
        )
    expected = expected_cases(contract)
    original = read(observed / "original/results.json")
    origin = json.loads(original)
    require(
        frame["sourceRevision"] == origin["sourceRevision"] == PIN
        and frame["classification"] == "tested"
        and frame["graphSha256"] == digest(read(base / "import/domain.ttl"))
        and frame["importLockSha256"] == digest(read(base / "import/import.lock"))
        and frame["cases"] == len(expected)
        and frame["originalSha256"] == digest(original)
        and frame["contractSha256"]
        == origin["contractSha256"]
        == digest(read(CONTRACT))
        and origin["cases"] == expected
        and frame["rustc"] == read(observed / "rustc.txt").decode().strip()
        and frame["wasmTools"] == read(observed / "wasm-tools.txt").decode().strip(),
        "framing report or original observations differ from contract",
    )
    harness_files = {
        p for p in (ROOT / "examples/wasm/framing").rglob("*") if p.is_file()
    }
    harness_files.update(
        ROOT / p
        for p in (
            "examples/wasm/Cargo.lock",
            "examples/wasm/host/Cargo.toml",
            "examples/wasm/toolchain.lock",
            "scripts/marmish/frame.sh",
            "scripts/marmish/frame_runner.py",
            "scripts/marmish/observations.py",
        )
    )
    require(
        frame["schemaVersion"] == 1
        and re.fullmatch(r"[0-9a-f]{64}", frame["hostSha256"])
        and frame["harnessSha256"]
        == {str(p.relative_to(ROOT)): digest(read(p)) for p in harness_files},
        "framing harness identity changed",
    )
    require(
        set(frame["builds"]) == {"regenerated", "mutant"},
        "missing framing build observations",
    )
    for name, projection in [
        ("regenerated", positive),
        ("mutant", base / "mutant-projection"),
    ]:
        dest = observed / name
        result = frame["builds"][name]
        require(
            result["artifacts"]
            == {
                p: digest(read(dest / p))
                for p in ("consumer", "component.wasm", "extracted.wit")
            }
            and result["projection"]
            == {
                p: digest(read(projection / p))
                for p in ("frame.rs", "world.wit", "projection.json")
            }
            and not result["wasmImports"]
            and read(dest / "extracted.wit") == read(observed / "expected.wit")
            and set(result["results"]) == {"native", "wasm"},
            "framing binary, target or ABI evidence differs",
        )
        required = (
            []
            if name == "regenerated"
            else [
                {
                    "case": "version-before-size",
                    "expected": {"error": "InvalidVersion"},
                    "actual": {"error": "SizeLimit"},
                }
            ]
        )
        for role in ("native", "wasm"):
            raw = read(dest / (role + ".json"))
            actual = json.loads(raw)
            differences = []
            for want, got in zip(expected, actual["cases"], strict=True):
                require(
                    got.keys() == want.keys()
                    and all(
                        got[k] == want[k] for k in ["id", "operation", "inputSha256"]
                    ),
                    f"wrong {name}/{role} contract case or input",
                )
                if got["observed"] != want["observed"]:
                    differences.append(
                        {
                            "case": want["id"],
                            "expected": want["observed"],
                            "actual": got["observed"],
                        }
                    )
            require(
                differences == actual["differences"] == required
                and result["results"][role]
                == {
                    "observationsSha256": digest(raw),
                    "caseCount": len(expected),
                    "differences": differences,
                },
                f"{name}/{role} observations disagree with independent outcomes",
            )


def summarize(directory, *, replay=False):
    def emit(path, value):
        data = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        if replay:
            require(read(path) == data, f"stale qualification aggregate: {path.name}")
        else:
            path.write_bytes(data)

    inventory_path = directory / "native/original/inventory/inventory.json.gz"
    raw_inventory = read(inventory_path)
    require(
        digest(raw_inventory)
        == read_json(ROOT / "examples/marmish/inventory-summary.json")[
            "inventorySha256"
        ],
        "original source inventory differs from published baseline",
    )
    inventory = json.loads(gzip.decompress(raw_inventory))
    reconstruction = verify_build_pair(directory / "rebuild", inventory)
    native = read_json(directory / "native/qualification.json")
    check_platforms(native["platforms"])
    require(
        native["verifierSha256"] == digest(read(HERE / "qualify_candidate.py")),
        "native report was not produced by the current verifier",
    )
    require(
        native
        == compare_archived(
            directory / "native",
            PIN,
            reconstruction["candidateCommit"],
            reconstruction["candidateTree"],
        ),
        "native summary disagrees with raw native observations",
    )
    wire = read_json(directory / "wire.json")
    contract = json.loads(read(CONTRACT))
    cases = contract["cases"]
    check_wire(contract, wire)
    check_frame(directory, contract)
    generated = reconstruction["generated"]
    residuals = coverage(
        inventory, generated, read_json(ROOT / "examples/marmish/gaps.json")
    )
    emit(directory / "coverage.json", residuals)
    summary = {
        "scope": "native Apple Silicon source-build comparison and framing component conformance",
        "originalCommit": native["originalCommit"],
        "candidateCommit": native["candidateCommit"],
        "candidateTree": reconstruction["candidateTree"],
        "independentBuilds": 2,
        "sourceFiles": reconstruction["sourceFiles"],
        "retainedFilesBeforeIndexUpdate": reconstruction["retainedFiles"],
        "regeneratedRust": sorted(generated),
        "regeneratedIndex": reconstruction["regeneratedOntology"],
        "lexicalSites": {
            k: residuals[k] for k in ("knownSites", "retainedSites", "regeneratedSites")
        },
        "compiledRuntimeDenominator": None,
        "platforms": native["platforms"],
        "nativeVerdict": native["status"],
        "newRegressions": native["regressions"],
        "frameCases": len(cases),
        "mixedWireExchanges": sum(len(r["exchanges"]) for r in wire["directions"]),
        "inputs": {
            name: digest((directory / name).read_bytes())
            for name in [
                "coverage.json",
                "wire.json",
                "frame/observations/report.json",
                "native/qualification.json",
                "rebuild/rebuild.json",
            ]
        },
        "wholeApplicationRegenerated": False,
        "universalBehaviorEquivalence": "open",
    }
    emit(directory / "qualification.json", summary)
    return summary


def verify(upstream, directory):
    directory.mkdir(parents=True, exist_ok=True)
    if any(directory.iterdir()):
        raise ValueError("verify requires an empty destination")
    rebuild(upstream, directory / "rebuild")
    candidate = directory / "rebuild/candidate"
    commands = [
        [
            sys.executable,
            "-B",
            HERE / "qualify_candidate.py",
            upstream,
            candidate,
            directory / "native",
        ],
        [
            HERE / "offline.sh",
            sys.executable,
            "-B",
            HERE / "wire.py",
            upstream,
            candidate,
            directory / "wire.json",
        ],
        [ROOT / "scripts/qualify-marmish.sh", "frame", upstream, directory / "frame"],
    ]
    with ThreadPoolExecutor(max_workers=len(commands)) as pool:
        jobs = [pool.submit(subprocess.run, cmd, cwd=ROOT) for cmd in commands]
        statuses = [job.result().returncode for job in jobs]
    if any(statuses[1:]) or statuses[0] not in (0, 3):
        raise ValueError(f"incomplete qualification commands: {statuses}")
    result = summarize(directory)
    if (
        statuses[0] != 0
        or result["newRegressions"]
        or result["nativeVerdict"]
        not in ("passed measured observations", "baseline failures remain")
    ):
        raise ValueError("native candidate regression; inspect qualification.json")
    print(f"Qualified {result['candidateCommit']} in {directory}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "upstream_or_replay",
        help="Upstream checkout for a new run, or 'replay' to check archived evidence",
    )
    parser.add_argument(
        "output",
        type=Path,
        help="Empty output directory or extracted archive root for replay",
    )
    args = parser.parse_args()
    try:
        if args.upstream_or_replay == "replay":
            result = summarize(args.output.resolve(), replay=True)
            require(
                not result["newRegressions"]
                and result["nativeVerdict"]
                in ("passed measured observations", "baseline failures remain"),
                "archived native qualification records a regression",
            )
            print(
                f"Verified archived qualification for {result['candidateCommit']}: {result['nativeVerdict']}"
            )
        else:
            verify(Path(args.upstream_or_replay).resolve(), args.output.resolve())
    except (
        OSError,
        ValueError,
        KeyError,
        TypeError,
        subprocess.SubprocessError,
    ) as exc:
        sys.exit(f"Marmish qualification failed: {exc}")
