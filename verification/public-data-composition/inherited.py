"""Replay predecessor fixtures using the already qualified compiler binary."""

import argparse
import json
import subprocess
from pathlib import Path

from verify import RUNTIME, require_pin, sha


def replay(root, binary, receipt_path):
    require_pin(root)
    receipt = json.loads(receipt_path.read_bytes())
    digest = sha(binary)
    if receipt["runtime"] != RUNTIME or receipt["binarySha256"] != digest:
        raise ValueError("compiler differs from the native composition receipt")
    baseline = root / "verification/structural-profile-composition/aarch64-apple-darwin.json"
    if sha(baseline) != receipt["predecessorTestReceiptSha256"]:
        raise ValueError("predecessor inventory differs from the composition receipt")
    paths = [entry["fixture"] for entry in json.loads(baseline.read_bytes())["fixtures"]]
    paths.append("verification/plain-value-choices/verify.py")
    observations = []
    for path in paths:
        fixture = root / path
        result = subprocess.run(["python3", "-B", fixture, binary], cwd=root,
                                check=True, stdout=subprocess.PIPE, text=True)
        observations.append(dict(path=path, fixtureSha256=sha(fixture), stdout=result.stdout.strip()))
    require_pin(root)
    if sha(binary) != digest:
        raise ValueError("compiler changed during replay")
    return dict(runtime=RUNTIME, host=receipt["host"], binarySha256=digest,
                compositionReceiptSha256=sha(receipt_path), runnerSha256=sha(Path(__file__)),
                fixtures=observations)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("root", "binary", "receipt", "result"):
        parser.add_argument(name, type=lambda value: Path(value).resolve())
    args = parser.parse_args()
    args.result.write_text(json.dumps(replay(args.root, args.binary, args.receipt), indent=2) + "\n")
