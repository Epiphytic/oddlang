"""Portable checks on the frozen table and its differential comparator."""
import base64
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import observations


class ObservationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.table = json.loads(observations.CONTRACT.read_bytes())
        cls.cases = {c["id"]: c for c in cls.table["cases"]}

    def test_raw_boundary_and_exact_wire(self):
        for key, n in [("encode-raw-limit", 16384), ("encode-raw-plus-one", 16385)]:
            case = self.cases[key]
            self.assertEqual(len(observations.expand(case["input"])), n)
        maximum = self.cases["encode-raw-limit"]
        wire = b'{"version":1,"payload":"' + base64.b64encode(bytes([255]) * 16384) + b'"}'
        self.assertEqual(observations.expand(maximum["expected"]["bytes"]), wire)
        extra = observations.expand(self.cases["decode-raw-plus-one"]["input"])
        self.assertEqual(len(base64.b64decode(json.loads(extra)["payload"], validate=True)), 16385)

    def test_json_boundaries_are_measured_in_octets(self):
        for n in [32768, 32769]:
            case = self.cases[f"json-{n}"]
            wire = observations.expand(case["input"])
            self.assertEqual(len(wire), n)
            self.assertEqual(json.loads(wire), {"version": 1, "payload": ""})
        self.assertEqual(self.cases["json-32769"]["expected"], {"error": "SizeLimit"})

    def test_payload_before_version_mutant_fails(self):
        case = self.cases["version-before-encoding"]
        self.assertEqual(case["expected"], {"error": "InvalidVersion"})
        wrong = subprocess.CompletedProcess([], 2, b"", b"InvalidEncoding\n")
        with patch.object(observations.subprocess, "run", return_value=wrong):
            with self.assertRaisesRegex(ValueError, "version-before-encoding"):
                observations.observe(Path("consumer"), case)


if __name__ == "__main__":
    unittest.main()
