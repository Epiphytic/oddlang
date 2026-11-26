import json
from pathlib import Path
import tarfile
import tempfile
import unittest

import original
import qualify_candidate as verify


class CandidateObservations(unittest.TestCase):
    def test_recovers_declared_tests_from_nested_harness_output(self):
        with tarfile.open(original.ARCHIVE, "r:gz") as arc:
            for name, expected in (("workspace", 920), ("installed", 918)):
                with self.subTest(name=name):
                    streams = [arc.extractfile(f"native/logs/{name}.{s}").read().decode()
                               for s in ("stdout", "stderr")]
                    rows = verify.test_outcomes(*streams)
                    self.assertEqual(len(rows), expected)

    def test_balanced_swap_preserves_both_differences(self):
        self.assertEqual(verify.deltas({"a": "ok", "b": "FAILED"}, {"a": "FAILED", "b": "ok"}), [
            {"test": "a", "original": "ok", "candidate": "FAILED"},
            {"test": "b", "original": "FAILED", "candidate": "ok"},
        ])

    def test_named_improvements_preserve_cargo_exit_and_each_previous_test_outcome(self):
        commands = [{"command": "workspace", "original": 101, "candidate": 0},
                    {"command": "marmish-worker-help", "original": 1, "candidate": 0},
                    {"command": "installed", "original": 0, "candidate": 101}]
        rows = verify.deltas({"fixed": "FAILED", "activated": "ignored, platform",
                              "broken": "ok"}, {"fixed": "ok", "activated": "ok", "broken": "FAILED", "new": "ok"})
        actual = verify.improvements(commands, {"workspace": {"differences": rows}})
        self.assertEqual(actual, [
            {"kind": "cargo-exit", "command": "workspace", "original": 101, "candidate": 0},
            {"kind": "test", "suite": "workspace", "test": "activated", "original": "ignored, platform", "candidate": "ok"},
            {"kind": "test", "suite": "workspace", "test": "fixed", "original": "FAILED", "candidate": "ok"},
        ])

    def test_truncated_test_log_rejected(self):
        with self.assertRaisesRegex(ValueError, "individual outcomes"):
            verify.test_outcomes("running 2 tests\ntest good ... ok\n"
                                 "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n",
                                 "     Running tests/demo.rs (/tmp/demo-0123456789abcdef)\n")

    def test_sdk_diagnostic_ignores_only_the_cache_suffix(self):
        warning = b"xcrun: error: couldn't create cache file '/var/folders/aa/bb/T/xcrun_db-Ab123456' (errno=Operation not permitted)\n"
        baseline = verify.sdk_diagnostics(warning)
        self.assertEqual(baseline, verify.sdk_diagnostics(warning.replace(b"Ab123456", b"XY654321")))
        for change in (warning.replace(b"not permitted", b"not found"), warning.replace(b"/aa/", b"/bb/"), warning * 2):
            self.assertNotEqual(baseline, verify.sdk_diagnostics(change))

    def test_missing_or_new_case_is_explicit(self):
        delta = verify.deltas({"old": "ignored"}, {"new": "ok"})
        self.assertEqual([x["original"] for x in delta], ["unobserved", "ignored"])
        self.assertEqual([x["candidate"] for x in delta], ["ok", "unobserved"])

    def test_local_build_paths_are_normalized_without_changing_semantics(self):
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp) / "meta"
            p.write_text(json.dumps({"target": "/private/tmp/m-a123/target", "features": ["installed-checks"]}))
            a = verify.cargo_metadata(p)
            p.write_text(json.dumps({"target": "/private/tmp/m-b456/target", "features": ["installed-checks"]}))
            self.assertEqual(verify.cargo_metadata(p), a)
            p.write_text(json.dumps({"target": "/private/tmp/m-b456/target", "features": []}))
            self.assertNotEqual(verify.cargo_metadata(p), a)
