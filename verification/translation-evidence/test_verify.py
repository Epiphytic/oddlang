"""Replay rejection tests. Supply a freshly verified report via ODDLANG_REPLAY_FIXTURE."""
import copy
import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path

import verify


class MutantWitnessTests(unittest.TestCase):
    def setUp(self):
        overflow = verify.ERROR + "DestinationOverflow"
        rows = {key: {"id": key, "owner": "", "balance": balance}
                for key, balance in (("debit", 1), ("credit", 2**63 - 1))}
        changed = copy.deepcopy(rows)
        changed["debit"]["balance"] = 0
        self.mutants = {
            "guardOrder": {
                "input": {"source": "", "destination": "", "amount": -1, "rows": {}},
                "expected": {"status": verify.ERROR + "SourceMissing", "rows": {}},
                "actual": {"status": verify.ERROR + "NonPositiveAmount", "rows": {}},
            },
            "earlyDebit": {
                "input": {"source": "debit", "destination": "credit", "amount": 1, "rows": rows},
                "expected": {"status": overflow, "rows": rows},
                "actual": {"status": overflow, "rows": changed},
            },
        }

    def test_both_witnesses(self):
        verify.verify_mutants(self.mutants)

    def test_equal_outcomes_do_not_disprove_guard_order(self):
        guard = self.mutants["guardOrder"]
        guard["actual"] = guard["expected"]
        with self.assertRaisesRegex(ValueError, "guard mutant"):
            verify.verify_mutants(self.mutants)

    def test_changed_owner_is_not_a_debit_witness(self):
        debit = self.mutants["earlyDebit"]
        debit["actual"]["rows"]["debit"]["owner"] = "changed"
        with self.assertRaisesRegex(ValueError, "premature state"):
            verify.verify_mutants(self.mutants)


@unittest.skipUnless(os.getenv("ODDLANG_REPLAY_FIXTURE"), "requires a collected translation report")
class ArchiveReplayTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(__file__).resolve().parents[2]
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.report = Path(self.temp.name) / "evidence"
        shutil.copytree(os.environ["ODDLANG_REPLAY_FIXTURE"], self.report)
        verify.check(self.root, self.report)

    def test_missing_report(self):
        (self.report / "proof/result.json").unlink()
        with self.assertRaises(FileNotFoundError):
            verify.check(self.root, self.report)

    def test_missing_summary(self):
        (self.report / "summary.json").unlink()
        with self.assertRaises(FileNotFoundError):
            verify.check(self.root, self.report)

    def test_stale_build(self):
        with (self.report / "generated/account.rs").open("a") as artifact:
            artifact.write("\n")
        with self.assertRaisesRegex(ValueError, "stale differential artifact: generated"):
            verify.check(self.root, self.report)

    def test_stale_proof_log(self):
        with (self.report / "proof/transfer_equivalence.log").open("a") as log:
            log.write("\n")
        with self.assertRaisesRegex(ValueError, "stale proof output"):
            verify.check(self.root, self.report)

    def test_unbounded_proof_claim(self):
        path = self.report / "proof/result.json"
        result = verify.read(path)
        result["unboundedKeysAndMaps"] = "proved over the recorded domain"
        path.write_text(json.dumps(result))
        with self.assertRaisesRegex(ValueError, "unboundedKeysAndMaps"):
            verify.check(self.root, self.report)

    def test_false_domain_promotion(self):
        path = self.report / "summary.json"
        summary = verify.read(path)
        summary["claims"]["arbitraryLedger"]["classification"] = "proved"
        path.write_text(json.dumps(summary))
        with self.assertRaisesRegex(ValueError, "differs from recomputed"):
            verify.check(self.root, self.report)

    def test_missing_outcome(self):
        path = self.report / "differential.json"
        result = verify.read(path)
        del result["outcomes"]["ok"]
        path.write_text(json.dumps(result))
        with self.assertRaisesRegex(ValueError, "reachable outcome"):
            verify.check(self.root, self.report)


if __name__ == "__main__":
    unittest.main()
