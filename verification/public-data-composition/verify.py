"""Replay the combined public-data compiler fixtures against one pinned executable."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

RUNTIME = "900cfa517d0136fa80c2b5e92ccf9b1a3b2a71ab"
FIXTURES = (
    "verification/observational-record-defaults/pointer_width.py",
    "verification/plain-value-choices/capabilities.py",
    "verification/shared-declarations/verify.py",
    "verification/serde-string-choice/verify.py",
    "verification/plain-value-choices/typescript_subset.py",
)
NODE_FIXTURE = "verification/plain-value-choices/typescript.py"


def sha(file):
    return hashlib.sha256(file.read_bytes()).hexdigest()


def output(args, root):
    return subprocess.check_output(args, cwd=root, text=True).strip()


def require_pin(root):
    if output(["git", "rev-parse", "HEAD"], root) != RUNTIME or output(
        ["git", "status", "--porcelain", "--untracked-files=all"], root
    ):
        raise ValueError("expected the clean pinned compiler checkout")


def observe(root, target, with_node):
    require_pin(root)
    if target.is_relative_to(root):
        raise ValueError("use an external Cargo target")
    cargo = ["cargo", "+1.96.1"]
    subprocess.run([*cargo, "build", "--locked", "--offline", "--bin", "oddlang",
                    "--target-dir", target], cwd=root, check=True)
    binary = target / "debug/oddlang"
    binary_sha = sha(binary)
    version = output(["rustc", "+1.97.1", "-vV"], root).splitlines()
    host, = [line[6:] for line in version if line.startswith("host: ")]
    cases = FIXTURES + ((NODE_FIXTURE,) if with_node else ())
    results = {}
    for name in cases:
        path = root / name
        results[name] = {
            "fixtureSha256": sha(path),
            "output": output(["python3", "-B", path, binary], root),
        }
    library_tests = output([*cargo, "test", "--offline", "--locked", "--lib", "--target-dir", target], root)
    passed = sorted(line for line in library_tests.splitlines()
                    if line.startswith("test ") and line.endswith(" ... ok"))
    baseline = root / "verification/structural-profile-composition/aarch64-apple-darwin.json"
    required = json.loads(baseline.read_bytes())["cargoTests"]["lib"]
    names = sorted(line for line in required if line.startswith("test ") and line.endswith(" ... ok"))
    if not names or passed != names:
        raise ValueError("library test names differ from the pinned predecessor")
    require_pin(root)
    if sha(binary) != binary_sha:
        raise ValueError("the compiler executable changed during qualification")
    receipt = {"runtime": RUNTIME, "binarySha256": binary_sha, "fixtureRustc": version,
               "compilerRustc": output(["rustc", "+1.96.1", "-vV"], root).splitlines(),
               "host": host, "runnerSha256": sha(Path(__file__)), "fixtures": results,
               "predecessorTestReceiptSha256": sha(baseline), "libraryTests": passed}
    if with_node:
        receipt.update(node=output(["node", "--version"], root),
                       typescript=output(["npx", "--offline", "--package=typescript@5.9.3", "tsc", "--version"], root))
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("framework", "target", "receipt"):
        parser.add_argument(arg, type=lambda x: Path(x).resolve())
    parser.add_argument("--with-node", action="store_true", help="also run the TypeScript consumer using Node and tsc")
    args = parser.parse_args()
    args.receipt.write_text(json.dumps(observe(args.framework, args.target, args.with_node), indent=2) + "\n")
