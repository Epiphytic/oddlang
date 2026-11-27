"""Recover and independently check the archived qualification reference."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile

from baseline import sha

ROOT = Path(__file__).resolve().parents[2]
ARCHIVE = ROOT / "examples/marmish/evidence/original.tar.gz"
DIGEST = "4c944d14b9260b2300a164cedda7f8721cb0e487efd1ef7e515c5a900f8fea32"
TREE = "c78b36ae6720565170bc6dce578531f58ab671c7"
TEST_SUMMARY = re.compile(
    r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"\d+ measured; (\d+) filtered out;", re.MULTILINE)


def outcomes(path):
    text = path.read_text()
    totals = Counter({"passed": 0, "failed": 0, "ignored": 0})
    for passed, failed, ignored, filtered in TEST_SUMMARY.findall(text):
        if filtered == "0":
            totals.update({"passed": int(passed), "failed": int(failed), "ignored": int(ignored)})
    names = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED$", text, re.MULTILINE)))
    return {"counts": dict(totals), "failedTests": names}


def recover(directory):
    if sha(ARCHIVE) != DIGEST:
        raise ValueError("published original archive digest mismatch")
    directory.mkdir(exist_ok=True, parents=True)
    if any(directory.iterdir()):
        raise ValueError("original archive needs empty destination")
    with tarfile.open(ARCHIVE, "r:gz") as archive:
        for entry in archive:
            rel = Path(entry.name)
            if rel.is_absolute() or ".." in rel.parts or not entry.isfile():
                raise ValueError("invalid member in original archive")
            target = directory / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as dest:
                dest.write(archive.extractfile(entry).read())
    subprocess.run(["cargo", "+1.97.1", "run", "--quiet", "--offline", "--locked",
                    "--manifest-path", ROOT / "verification/marmish-inventory/Cargo.toml",
                    "--bin", "marmish-receipt", "--", "check", directory], check=True)
    native = json.loads((directory / "native/baseline.json").read_bytes())
    if native["tree"] != TREE:
        raise ValueError("original Git tree changed")
    summary = json.loads((directory / "inventory/inventory-summary.json").read_bytes())
    frozen = json.loads((ROOT / "examples/marmish/inventory-summary.json").read_bytes())
    if summary != frozen:
        raise ValueError("original inventory differs from checked summary")
    observed = {name: outcomes(directory / "native/logs" / f"{name}.stdout")
                for name in ("workspace", "installed")}
    for name, expected in {"workspace": (906,9,5), "installed": (907,8,3)}.items():
        counts = observed[name]["counts"]
        if tuple(counts[k] for k in ("passed","failed","ignored")) != expected:
            raise ValueError(f"published {name} summary disagrees with test transcripts")
    return observed


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("empty_directory", type=Path)
    args = parser.parse_args()
    print(json.dumps(recover(args.empty_directory), indent=2, sort_keys=True))
