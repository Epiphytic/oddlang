"""Recompute the evidence inventory and check each claim against its witnesses."""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path


LEDGER = "urn:oddlang:ledger:"
ERROR = LEDGER + "error:"
OUTCOMES = {"ok"} | {
    ERROR + name for name in (
        "SourceMissing", "DestinationMissing", "SameAccount", "NonPositiveAmount",
        "InsufficientFunds", "DestinationOverflow",
    )
}
FIXTURE = Path("examples/rust-import/ledger")
HARNESS = Path("tests/rust_translation.rs")
ORACLE = Path("tests/support/ledger_oracle.rs")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def inventory(base, names):
    return {str(name): digest(base / name) for name in sorted(names)}


def verify_lock(root, report):
    fixture = root / FIXTURE
    lock = read(report / "import/import.lock")
    overlays = {"avro.ttl", "fields.ttl", "rust.ttl", "sql.ttl"}
    graph = overlays | {"domain.ttl", "operations.ttl", "source-map.json"}
    require(lock["inputs"] == inventory(fixture, ["Cargo.toml", "Cargo.lock", "src/lib.rs"]),
            "locked source inputs changed")
    frozen = {}
    for line in (fixture / "original.sha256").read_text().splitlines():
        checksum, name = line.split("  ", 1)
        frozen[name] = checksum
    require(frozen == lock["inputs"], "frozen source digest disagrees with import lock")
    require(lock["overlays"] == inventory(fixture / "overlays", overlays), "locked overlays changed")
    require(lock["files"] == inventory(report / "import", graph), "imported artifacts changed")
    semantic = {k: v for k, v in lock["files"].items() if k.endswith(".ttl")}
    canonical = (json.dumps(semantic, sort_keys=True, indent=2) + "\n").encode()
    require(lock["graphSha256"] == hashlib.sha256(canonical).hexdigest(),
            "graph digest disagrees with the imported ontology")
    require(lock["mappingSha256"] == digest(fixture / "mapping.toml"), "mapping changed")
    require(lock["profile"] == "ledger-v1" and lock["version"] == 1 and lock["cleanup"] is None,
            "unexpected import profile")
    require(digest(report / "original.pin") == digest(root / FIXTURE.with_suffix(".baseline")),
            "original baseline changed")
    require(lock["target"] and "commit-hash: " in lock["compiler"] and
            lock["flags"] == ["-C", "overflow-checks=yes"], "missing production configuration")
    return lock


def verify_mutants(mutants):
    guard = mutants["guardOrder"]
    entry = guard["input"]
    require(entry["source"] not in entry["rows"] and entry["amount"] < 0,
            "guard witness lacks overlapping missing-source and negative-amount guards")
    require(guard["expected"] == {"status": ERROR + "SourceMissing", "rows": entry["rows"]} and
            guard["actual"] == {"status": ERROR + "NonPositiveAmount", "rows": entry["rows"]},
            "guard mutant witness does not discriminate error order")
    debit = mutants["earlyDebit"]
    entry = debit["input"]
    rows = entry["rows"]
    src = entry["source"]
    dst = entry["destination"]
    amount = entry["amount"]
    require(src != dst and 0 < amount <= rows[src]["balance"] and
            rows[dst]["balance"] + amount > 2**63 - 1, "invalid overflow witness")
    expected = {"status": ERROR + "DestinationOverflow", "rows": rows}
    changed = dict(rows)
    changed[src] = dict(rows[src], balance=rows[src]["balance"] - amount)
    require(debit["expected"] == expected and
            debit["actual"] == dict(expected, rows=changed),
            "early debit witness does not show the premature state change")


def verify_differential(root, report, hashes, lock):
    diff = read(report / "differential.json")
    require(diff["classification"] == "tested" and diff["schemaVersion"] == 1 and
            diff["role"] == "native-rust", "unexpected differential claim")
    require(diff["operation"] == LEDGER + "TransferFunds", "wrong differential operation")
    hashes = {
        **hashes,
        "adapter": digest(root / "tests/support/rust_translation/runner.rs"),
        "corpusSource": digest(root / "tests/support/rust_translation/corpus.rs"),
        "graph": lock["graphSha256"], "importLock": digest(report / "import/import.lock"),
    }
    for key, name in {"cases": "cases", "reorderedSource": "reordered-source.rs",
                      "reorderedGenerated": "reordered-generated.rs",
                      "earlyDebitGenerated": "early-debit.rs"}.items():
        hashes[key] = digest(report / "corpus" / name)
    require(set(diff["artifacts"]) == set(hashes), "incomplete differential artifact inventory")
    for name, value in hashes.items():
        require(diff["artifacts"][name] == value, f"stale differential artifact: {name}")
    counts = diff["outcomes"]
    require(set(counts) == OUTCOMES and all(type(n) is int and n > 0 for n in counts.values()),
            "missing reachable outcome witness")
    lines = (report / "corpus/cases").read_bytes().splitlines()
    require(sum(counts.values()) == len(lines) == diff["cases"], "case count disagrees with corpus")
    require(0 < diff["randomCases"] < len(lines), "missing seeded or hand-selected cases")
    require(isinstance(diff["seed"], str) and int(diff["seed"], 16) >= 0, "missing corpus seed")
    cleanup = diff["cleanup"]
    require(cleanup["originalSha256"] == hashes["original"] and
            cleanup["effectiveSha256"] != hashes["original"] and
            all(len(bytes.fromhex(cleanup[k])) == 32 for k in ("effectiveSha256", "patchSha256"))
            and cleanup["cases"] == diff["cases"], "edited-source comparison incomplete")
    for key, name in {"effectiveSha256": "effective.rs", "generatedSha256": "cleaned.rs",
                      "patchSha256": "cleanup.patch"}.items():
        require(cleanup[key] == digest(report / "corpus" / name), f"stale cleanup artifact: {name}")
    verify_mutants(diff["mutants"])
    for mutant in diff["mutants"].values():
        entry = mutant["input"]
        fields = [entry["source"].encode().hex(), entry["destination"].encode().hex(), str(entry["amount"])]
        for key, row in sorted(entry["rows"].items()):
            require(key == row["id"], "invalid witness key")
            fields.extend([key.encode().hex(), row["owner"].encode().hex(), str(row["balance"])])
        require("\t".join(fields).encode() in lines, "mutant input absent from archived corpus")
    return diff


def verify_success(log, name, covers):
    require(f"Checking harness {name}..." in log and
            re.search(r"\*\* 0 of [1-9][0-9]* failed", log) and
            f"{covers} of {covers} cover properties satisfied" in log and
            "VERIFICATION:- SUCCESSFUL" in log and "Complete - 1 successfully verified harnesses, 0 failures" in log
            and "undetermined" not in log, f"incomplete successful proof: {name}")


def verify_proof(root, report, hashes):
    folder = report / "proof"
    proof = read(folder / "result.json")
    proved = "proved over the recorded domain"
    pins = {
        "version": 1, "kani": "0.68.0", "cbmc": "6.11.0", "solver": "cadical",
        "unwind": 64, "flags": ["-C", "overflow-checks=yes"], "timeoutSeconds": 90,
        "originalSha256": hashes["original"], "generatedSha256": hashes["generated"],
        "transfer_equivalence": proved, "guard_mutant": "disproved", "guard_replay": "tested",
        "unboundedKeysAndMaps": "inconclusive",
        "scope": "empty map, source s, destination s or d, arbitrary signed i64 amount",
        "populatedScope": "s/d/z rows, owners a/b/c, arbitrary admissible i64 balances, source s/x, destination s/x/d, arbitrary i64 amount",
        "witness": {"rows": [], "source": "s", "destination": "d", "amount": -1,
                    "expected": "SourceMissing", "actual": "NonPositiveAmount"},
    }
    for key, expected in pins.items():
        require(proof[key] == expected, f"invalid proof configuration or claim: {key}")
    require("commit-hash: 8925ea358a0f265ca61026aadc7ecc506c545cbe" in proof["proofCompiler"],
            "wrong proof compiler")
    package_files = ["Cargo.toml", "Cargo.lock", "src/lib.rs", "scripts/prove.sh",
                     "scripts/runner.py", "scripts/setup.sh"]
    require(proof["packageSha256"] == inventory(root / "verification/rust-translation", package_files),
            "stale proof package")
    files = ["transfer_equivalence.log", "populated_equivalence.log", "guard_mutant.log",
             "guard_replay.log", "guard_witness.rs", "guard_mutant.rs"]
    require(proof["artifactsSha256"] == inventory(folder, files), "stale proof output")
    require(proof["mutantSha256"] == digest(folder / "guard_mutant.rs"), "stale guard mutant")
    # Both mutations independently move the same production guard to first priority.
    require((folder / "guard_mutant.rs").read_bytes() == (report / "corpus/reordered-generated.rs").read_bytes(),
            "proof mutant differs from independently regenerated guard-order mutant")
    verify_success((folder / "transfer_equivalence.log").read_text(), "transfer_equivalence", 1)
    populated = (folder / "populated_equivalence.log").read_text()
    if proof["populated_equivalence"] == proved:
        verify_success(populated, "populated_equivalence", 7)
    else:
        require(proof["populated_equivalence"] == "inconclusive" and
                "Checking harness populated_equivalence..." in populated and "CBMC timed out." in populated
                and "Failed Checks:" not in populated, "unexplained populated proof result")
    mutant = (folder / "guard_mutant.log").read_text()
    require("Checking harness guard_priority_counterexample..." in mutant and
            re.search(r"\*\* 1 of [1-9][0-9]* failed", mutant) and
            'Failed Checks: "exact named result differs"' in mutant and "undetermined" not in mutant and
            "VERIFICATION:- FAILED" in mutant, "unexpected guard mutant failure")
    witness = (folder / "guard_witness.rs").read_text()
    name = re.search(r"fn (kani_concrete_playback_guard_priority_counterexample_\d+)\(\)", witness)
    replay = (folder / "guard_replay.log").read_text()
    require(name and "vec![255, 255, 255, 255, 255, 255, 255, 255]" in witness and
            f"test {name[1]} ... FAILED" in replay and "test result: FAILED. 0 passed; 1 failed" in replay
            and "\nexact named result differs\n" in replay, "missing concrete Kani replay")
    return {
        "emptyMap": {"classification": proved, "domain": proof["scope"]},
        "populatedMap": {"classification": proof["populated_equivalence"], "domain": proof["populatedScope"]},
        "guardKaniCounterexample": {"classification": "disproved", "witness": proof["witness"]},
        "guardConcretePlayback": {"classification": "tested", "witness": "proof/guard_witness.rs"},
    }


def summarize(root, report):
    lock = verify_lock(root, report)
    hashes = {
        "original": digest(root / FIXTURE / "src/lib.rs"),
        "generated": digest(report / "generated/account.rs"),
        "oracle": digest(root / ORACLE),
        "harness": digest(root / HARNESS),
    }
    diff = verify_differential(root, report, hashes, lock)
    proof_claims = verify_proof(root, report, hashes)
    inputs = [
        "Cargo.toml", "Cargo.lock", str(HARNESS), str(ORACLE),
        "docs/language/ledger-core.md", "scripts/verify-rust-translation.sh",
        "verification/rust-translation/README.md",
        "verification/translation-evidence/verify.py",
        str(FIXTURE / "original.sha256"), str(FIXTURE.with_suffix(".baseline")),
    ]
    build_files = ["generated/account.rs", "generated/account.avsc", "generated/schema.sql"]
    artifacts = ["original.pin", "import/import.lock", "differential.json", "proof/result.json"]
    artifacts.extend("import/" + name for name in lock["files"])
    artifacts.extend(build_files)
    return {
        "schemaVersion": 1,
        "contract": {"profile": lock["profile"], "entity": LEDGER + "Account",
                     "operation": LEDGER + "TransferFunds", "outcomes": sorted(OUTCOMES)},
        "graphSha256": lock["graphSha256"],
        "production": {k: lock[k] for k in ("compiler", "target", "flags")},
        "observations": ["success or exact first named error", "all map keys",
                         "id, owner and balance of every poststate row, including bystanders"],
        "roles": ["native-rust"],
        "proofAssumptions": [
            "Single-threaded calls, successful allocation and completed Rust observations.",
            "Kani MIR and standard-library models, abort on panic, checked overflow and unwinding assertions.",
            "The proof covers Kani's pinned compiler and target. Equivalence with native compilation remains open.",
        ],
        "sourceInputs": inventory(root, inputs),
        "reportFiles": inventory(report, artifacts),
        "claims": {
            "nativeCorpus": {"classification": "tested", "cases": diff["cases"], "seed": diff["seed"]},
            "editedSourceCorpus": {"classification": "tested", "cases": diff["cleanup"]["cases"]},
            "guardOrderMutant": {"classification": "disproved", "witness": "differential.json#mutants/guardOrder"},
            "earlyDebitMutant": {"classification": "disproved", "witness": "differential.json#mutants/earlyDebit"},
            **proof_claims,
            "arbitraryLedger": {
                "classification": "inconclusive",
                "domain": "All valid ledgers, arbitrary map sizes, keys and owner strings.",
                "openObligations": [
                    "Key renaming preserves presence, value, and all observable results in both bodies.",
                    "Adding arbitrary bystanders preserves addressed results and every bystander.",
                    "The proof configuration models the production toolchain for the claimed domain.",
                ],
            },
        },
    }


def check(root, report, write=False):
    result = summarize(root, report)
    path = report / "summary.json"
    data = (json.dumps(result, sort_keys=True, indent=2, ensure_ascii=False) + "\n").encode()
    if write:
        # Evidence collection occurs in a fresh directory. Never refresh a stale archive.
        with path.open("xb") as output:
            output.write(data)
    require(path.read_bytes() == data, "summary.json differs from recomputed evidence")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("report", type=Path)
    parser.add_argument("--write", action="store_true", help="seal a newly collected report")
    args = parser.parse_args()
    try:
        check(args.root, args.report, args.write)
    except (ValueError, KeyError, OSError, TypeError, IndexError) as exc:
        sys.exit(f"invalid translation evidence: {exc}")
