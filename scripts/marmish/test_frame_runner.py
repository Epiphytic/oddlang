"""Reject incomplete and ambiguous output at the client observation boundary."""
import subprocess
import unittest
from unittest.mock import patch

from frame_runner import batch


class FrameBatchTests(unittest.TestCase):
    cases = [{"id": "binary", "operation": "decode", "input": [{"hex": "00ff"}]}]

    def read(self, wire):
        with patch("frame_runner.subprocess.run", return_value=subprocess.CompletedProcess([], 0, wire)) as run:
            result = batch(["consumer"], self.cases)
            self.assertEqual(run.call_args.kwargs["input"], b"D\x02\x00\x00\x00\x00\xff")
            return result[0]["observed"]

    def test_bytes_and_named_errors_remain_distinct(self):
        self.assertEqual(self.read(b"B\x04\0\0\0Oops"), {"bytesHex": "4f6f7073"})
        self.assertEqual(self.read(b"F\x04\0\0\0Oops"), {"error": "Oops"})

    def test_missing_truncated_and_extra_results_fail(self):
        for wire in [b"", b"B\x01\0\0\0", b"B\0\0\0\0extra"]:
            with self.subTest(wire=wire), self.assertRaises(ValueError):
                self.read(wire)
