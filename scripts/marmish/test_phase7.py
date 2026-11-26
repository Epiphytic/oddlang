import copy
import json
from pathlib import Path
import tempfile
import unittest

from observations import CONTRACT, expand
from phase7 import (
    check_generated_module,
    check_platforms,
    check_source_rows,
    check_wire,
)
from prepare import digest
from receipt import expected_cases


class Phase7ReplayTests(unittest.TestCase):
    def setUp(self):
        self.contract = json.loads(CONTRACT.read_bytes())
        exchanges = [
            {
                "id": case["id"],
                "rawSha256": digest(expand(case["input"])),
                "wireSha256": digest(expand(case["expected"]["bytes"])),
            }
            for case in self.contract["cases"]
            if case["operation"] == "encode" and "bytes" in case["expected"]
        ]
        self.wire = {
            "cases": expected_cases(self.contract),
            "publicApi": {"original": "tested", "candidate": "tested"},
            "directions": [
                {"writer": w, "reader": r, "exchanges": copy.deepcopy(exchanges)}
                for w, r in [("original", "candidate"), ("candidate", "original")]
            ],
        }

    def test_one_changed_wire_byte_invalidates_qualification(self):
        check_wire(self.contract, self.wire)
        witness = self.wire["directions"][1]["exchanges"][0]
        row = next(r for r in self.wire["cases"] if r["id"] == witness["id"])
        body = bytearray.fromhex(row["observed"]["bytesHex"])
        body[0] ^= 1
        row["observed"]["bytesHex"] = body.hex()
        witness["wireSha256"] = digest(body)
        with self.assertRaises(ValueError):
            check_wire(self.contract, self.wire)

    def test_exchanges_require_both_directions_and_exact_case_ids(self):
        for field in ("writer", "id"):
            wire = copy.deepcopy(self.wire)
            if field == "writer":
                wire["directions"][0]["writer"] = "candidate"
            else:
                wire["directions"][0]["exchanges"][0]["id"] = "substituted"
            with self.assertRaises(ValueError):
                check_wire(self.contract, wire)

    def test_missing_generated_file_invalidates_producer(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory) / "frame.rs"
            p.write_bytes(b"emitted")
            check_generated_module(p, digest(b"emitted"))
            p.unlink()
            with self.assertRaises((OSError, ValueError)):
                check_generated_module(p, digest(b"emitted"))

    def test_retained_digest_matches_original_inventory(self):
        original = {
            "fixture.dat": {
                "mode": "100644",
                "sha256": digest(b"original"),
                "gitBlob": "1" * 40,
            }
        }
        row = {
            "path": "fixture.dat",
            "mode": "100644",
            "sha256": digest(b"original"),
            "producer": "retained",
        }
        check_source_rows([row], original, set())
        row["sha256"] = digest(b"changed")
        with self.assertRaises(ValueError):
            check_source_rows([row], original, set())

    def test_missing_platform_record_is_rejected(self):
        platforms = {
            "aarch64-apple-darwin": "observed",
            "x86_64-unknown-linux-gnu": "unobserved",
            "aarch64-unknown-linux-gnu": "unobserved",
        }
        check_platforms(platforms)
        del platforms["aarch64-unknown-linux-gnu"]
        with self.assertRaises(ValueError):
            check_platforms(platforms)

    def test_native_exit_three_fails_even_with_a_passing_summary(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        from phase7 import verify

        def runner(command, **_):
            return SimpleNamespace(
                returncode=(
                    3
                    if command[1:3] and str(command[2]).endswith("qualify_candidate.py")
                    else 0
                )
            )

        with tempfile.TemporaryDirectory() as d, patch("phase7.rebuild"), patch(
            "phase7.subprocess.run", side_effect=runner
        ), patch(
            "phase7.summarize",
            return_value={
                "nativeVerdict": "passed measured observations",
                "newRegressions": [],
            },
        ):
            with self.assertRaisesRegex(ValueError, "native candidate regression"):
                verify(Path(d), Path(d) / "out")

    def test_retained_site_outside_workspace_has_an_explicit_gap_error(self):
        from phase7 import coverage

        fixture = {
            "cargo": {"crates": {}},
            "symbols": [{"classification": "retained-native", "path": "other"}],
        }
        with self.assertRaisesRegex(
            ValueError, "retained path has no workspace package: other"
        ):
            coverage(fixture, set())

    def test_second_independent_build_is_verified_from_its_own_artifacts(self):
        from unittest.mock import patch
        from phase7 import (
            ONTOLOGY,
            PIN,
            PRODUCERS,
            git_object,
            tree_hash,
            verify_build_pair,
        )

        def save(path, value):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(value))
            return digest(path.read_bytes())

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            builds = root / "rebuild"
            frozen = root / "examples/marmish"
            inventory = {"files": {}}
            generated = {}
            entries = {}
            rows = []
            # Independent small blob names and a real canonical Git tree ID.
            for name, module in PRODUCERS.items():
                path = f"crates/marmish-core/src/{module}"
                original = b"original " + module.encode()
                emitted = b"emitted " + module.encode()
                before = git_object("blob", original)
                after = git_object("blob", emitted)
                inventory["files"][path] = {
                    "mode": "100644",
                    "sha256": digest(original),
                    "gitBlob": before,
                }
                entries[path] = ("100644", after)
                generated[path] = {
                    "originalBlob": before,
                    "candidateBlob": after,
                    "sha256": digest(emitted),
                }
                rows.append(
                    f'[[files]]\npath="{path}"\nmode="100644"\nproducer="generated"\nsha256="{digest(emitted)}"\n'
                )
                for run in ("first", "second"):
                    base = builds / run
                    imported = base / name
                    projected = base / (name + "-projection")
                    imported.mkdir(parents=True)
                    projected.mkdir()
                    (imported / "domain.ttl").write_bytes(b"fixture graph")
                    (imported / "source-map.json").write_bytes(b"{}")
                    graph = digest(b"fixture graph")
                    lock = save(
                        imported / "import.lock",
                        {
                            "graphSha256": graph,
                            "files": {
                                "domain.ttl": graph,
                                "source-map.json": digest(b"{}"),
                            },
                        },
                    )
                    (projected / module).write_bytes(emitted)
                    receipt = save(
                        projected / "projection.json",
                        {
                            "version": 1,
                            "graphSha256": graph,
                            "files": {module: digest(emitted)},
                        },
                    )
                    release = {
                        "graphSha256": graph,
                        "importSha256": lock,
                        "projectionSha256": receipt,
                    }
                    if name == "framing":
                        (projected / "world.wit").write_bytes(b"world fixture {}")
                        release["witSha256"] = digest(b"world fixture {}")
                    save(frozen / (name + "-release.json"), release)
            manifest = "".join(rows)
            manifest_hash = digest(manifest.encode())
            (frozen / "assembly-v2.sha256").write_text(manifest_hash + "\n")
            result = {
                "assemblySha256": manifest_hash,
                "sourceFiles": len(rows),
                "retainedFiles": 0,
                "generated": generated,
                "stagedAssemblyTree": tree_hash(entries),
                "regeneratedOntology": sorted(ONTOLOGY),
                "candidateCommit": "1" * 40,
                "candidateTree": "2" * 40,
            }
            save(
                builds / "rebuild.json",
                {"originalCommit": PIN, "independentBuilds": 2, **result},
            )
            for run in ("first", "second"):
                base = builds / run
                save(base / "result.json", result)
                (base / "project.toml").write_text(manifest)
                for verb in ("check", "update"):
                    (base / f"ontology-{verb}.txt").write_text(
                        f"code ontology {verb}: fixture counts\n"
                    )
            with patch("phase7.ROOT", root):
                verify_build_pair(builds, inventory)
                (builds / "second/facade-projection/lib.rs").unlink()
                with self.assertRaisesRegex(
                    ValueError,
                    "missing or redirected evidence.*second/facade-projection/lib.rs",
                ):
                    verify_build_pair(builds, inventory)
