"""Exercise CLI evidence validation independently of the native build."""
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cli_observations import verify_cli
from observations import CONTRACT, REVISION


class FrozenCliTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        logs = self.root / "logs"
        logs.mkdir()
        rows = [{"name": "release", "exitCode": 0, "status": "passed", "timedOut": False,
                 "argv": ["cargo", "build", "--locked", "--offline", "--release", "-p", "marmish-native", "--bins"]}]
        contract = json.loads(CONTRACT.read_bytes())
        bins = {}
        for case in contract["cli"]["cases"]:
            tool, flag = case["binary"], case["argument"]
            # Synthetic help bytes keep this reader test independent of a cached native executable.
            out = b"" if tool == "marmish-worker" else b"synthetic help\n" if flag == "--help" else b"marmish-native 0.3.1\n"
            name = tool + "-" + flag[2:]
            row = {"name": name, "argv": [f"$RUN/target/release/{tool}", flag],
                   "exitCode": case["exitCode"], "timedOut": False,
                   "status": "failed" if case["exitCode"] else "passed"}
            for stream, value in (("stdout", out), ("stderr", b"")):
                path = Path("logs") / (name + "." + stream)
                (self.root / path).write_bytes(value)
                sha = hashlib.sha256(value).hexdigest()
                case[stream] = {"byteLength": len(value), "sha256": sha}
                row[stream], row[stream + "Sha256"] = str(path), sha
            bins[tool] = {"size": 1, "sha256": "a" * 64}
            rows.append(row)
        self.native = {"revision": REVISION, "commands": rows, "binaries": bins}
        self.contract = self.root / "observations.json"
        self.contract.write_text(json.dumps(contract))

    def verify(self):
        with patch("cli_observations.CONTRACT", self.contract):
            verify_cli(self.native, self.root)

    def test_six_probes(self):
        self.verify()

    def test_worker_rejection_required(self):
        row = next(r for r in self.native["commands"] if r["name"] == "marmish-worker-help")
        row.update(exitCode=0, status="passed")
        with self.assertRaisesRegex(ValueError, "unexpected CLI exit"):
            self.verify()

    def test_changed_help_with_matching_log_hash_fails(self):
        row = next(r for r in self.native["commands"] if r["name"] == "marmish-help")
        altered = b"new help text\n"
        (self.root / row["stdout"]).write_bytes(altered)
        row["stdoutSha256"] = hashlib.sha256(altered).hexdigest()
        with self.assertRaisesRegex(ValueError, "unexpected CLI bytes"):
            self.verify()

    def test_missing_product_version(self):
        del self.native["commands"][1]
        with self.assertRaisesRegex(ValueError, "missing or incorrect CLI command"):
            self.verify()

    def test_failed_release_cannot_qualify(self):
        self.native["commands"][0].update(exitCode=1, status="failed")
        with self.assertRaisesRegex(ValueError, "release build failed"):
            self.verify()
