import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import receipt


class BaselineReceiptTests(unittest.TestCase):
    def setUp(self):
        cli = patch.object(receipt, "verify_cli")
        cli.start()
        self.addCleanup(cli.stop)
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        for role in ("native", "inventory", "observations"):
            (self.root / role).mkdir()
        logs = self.root / "native/logs"
        logs.mkdir()
        commands = []
        for name in ("ontology", "metadata", "installed-metadata", "workspace", "workspace-ignored",
                     "installed", "installed-ignored", "release", "cfg", "rustc", "cargo"):
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
        observation = {"sourceRevision": receipt.REVISION, "contractSha256": receipt.sha(receipt.CONTRACT),
                       "cases": receipt.expected_cases(json.loads(receipt.CONTRACT.read_bytes()))}
        (self.root / "observations/results.json").write_text(json.dumps(observation))

    def test_replay_detects_edited_observation(self):
        receipt.check(self.root, write=True)
        receipt.check(self.root)
        path = self.root / "observations/results.json"
        row = json.loads(path.read_text())
        row["changed"] = True
        path.write_text(json.dumps(row))
        with self.assertRaisesRegex(ValueError, "changed after collection"):
            receipt.check(self.root)
        with self.assertRaises(FileExistsError):
            receipt.check(self.root, write=True)

    def test_wrong_revision(self):
        (self.root / "native/baseline.json").write_text('{"revision":"wrong","commands":[1]}')
        with self.assertRaisesRegex(ValueError, "missing pinned native observations"):
            receipt.check(self.root, write=True)

    def test_reject_symlink(self):
        (self.root / "native/linked.log").symlink_to(self.root / "native/baseline.json")
        with self.assertRaisesRegex(ValueError, "unexpected symlink"):
            receipt.check(self.root, write=True)
