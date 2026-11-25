import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import receipt


class BaselineReceiptTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        for role in ("native", "inventory", "observations"):
            (self.root / role).mkdir()
        logs = self.root / "native/logs"
        logs.mkdir()
        commands = []
        for name in ("ontology", "metadata", "workspace", "installed", "release"):
            row = {"name": name, "exitCode": 0, "status": "passed"}
            for stream in ("stdout", "stderr"):
                path = logs / f"{name}.{stream}"
                path.write_bytes(b"")
                row[stream] = "logs/" + path.name
                row[stream + "Sha256"] = receipt.sha(path)
            commands.append(row)
        native = {"revision": receipt.REVISION, "commands": commands}
        (self.root / "native/baseline.json").write_text(json.dumps(native))
        archive = self.root / "inventory/inventory.json.gz"
        archive.write_bytes(b"inventory")
        summary = {"gitRevision": receipt.REVISION, "inventorySha256": receipt.sha(archive)}
        (self.root / "inventory/inventory-summary.json").write_text(json.dumps(summary))
        observation = {"sourceRevision": receipt.REVISION}
        (self.root / "observations/results.json").write_text(json.dumps(observation))

    def run_check(self, *args):
        return subprocess.run([sys.executable, receipt.__file__, self.root, *args],
                              text=True, capture_output=True)

    def test_replay_detects_edited_observation(self):
        self.assertEqual(self.run_check("--write").returncode, 0)
        self.assertEqual(self.run_check().returncode, 0)
        (self.root / "observations/results.json").write_text(json.dumps({"sourceRevision":receipt.REVISION, "changed":True}))
        self.assertIn("changed after collection", self.run_check().stderr)
        self.assertNotEqual(self.run_check("--write").returncode, 0)

    def test_wrong_revision(self):
        (self.root / "native/baseline.json").write_text('{"revision":"wrong","commands":[1]}')
        self.assertIn("missing pinned native observations", self.run_check("--write").stderr)

    def test_reject_symlink(self):
        (self.root / "native/linked.log").symlink_to(self.root / "native/baseline.json")
        self.assertIn("unexpected symlink", self.run_check("--write").stderr)
