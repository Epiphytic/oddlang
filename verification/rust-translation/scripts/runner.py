"""Stage exact input bytes, run Kani, and validate the scoped claims."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

PACKAGE = Path(__file__).resolve().parents[1]
TIMEOUT = 90
HARNESS = "transfer_equivalence"
MUTANT = "guard_priority_counterexample"
ASSERTION = "exact named result differs"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def version(args):
    return subprocess.check_output(args, text=True).strip()


def stage(path, expected, dest):
    data = Path(path).read_bytes()
    require(digest(data) == expected, f"digest mismatch: {path}")
    dest.write_bytes(data)


def mutate(path):
    lines = path.read_text().splitlines(keepends=True)
    def position(name):
        match = [i for i, line in enumerate(lines)
                 if f'return Err("urn:oddlang:ledger:error:{name}");' in line]
        require(len(match) == 1, f"expected one emitted guard for {name}")
        return match[0]
    first, moved = position("SourceMissing"), position("NonPositiveAmount")
    require(first < moved, "unexpected emitted guard order")
    lines.insert(first, lines.pop(moved))
    path.write_text("".join(lines))
    return digest(path.read_bytes())


def verify(stage_dir, output, name, args):
    path = output / f"{name}.log"
    with path.open("wb") as stream:
        run = subprocess.run(args, cwd=stage_dir, stdout=stream, stderr=subprocess.STDOUT,
                             timeout=TIMEOUT + 90)
    return run.returncode, path.read_text()


def kani(harness, *extra):
    return ["cargo", "kani", "--harness", harness, "--exact",
            "--solver", "cadical", "-Z", "unstable-options", "--harness-timeout",
            f"{TIMEOUT}s", "--output-format", "terse", *extra]


def successful(code, log, covers):
    require(code == 0 and "VERIFICATION:- SUCCESSFUL" in log and
            f"{covers} of {covers} cover properties satisfied" in log and
            re.search(r"\*\* 0 of [1-9][0-9]* failed", log) and "undetermined" not in log,
            "Kani did not discharge all safety and cover checks")


def main():
    orig, gen, ohash, ghash, out = sys.argv[1:]
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    result_file = out / "result.json"
    result_file.unlink(missing_ok=True)
    backend = version(["cargo", "kani", "--version"]).splitlines()
    require(backend == ["Kani Rust Verifier 0.68.0 (cargo plugin)", "CBMC 6.11.0"],
            f"unexpected verifier versions: {backend}")
    # Kani resolves its own compiler in its installed release bundle.
    compiler = Path(os.environ.get("KANI_HOME", Path.home() / ".kani" / "kani-0.68.0"))
    rustc = version([compiler / "toolchain/bin/rustc", "-vV"])
    require("commit-hash: 8925ea358a0f265ca61026aadc7ecc506c545cbe" in rustc,
            "unexpected proof compiler")
    hashes = {rel: digest((PACKAGE / rel).read_bytes()) for rel in [
        "Cargo.toml", "Cargo.lock", "src/lib.rs", "scripts/prove.sh", "scripts/runner.py",
        "scripts/setup.sh"]}
    with tempfile.TemporaryDirectory(prefix="kani-stage-", dir=out) as tmp:
        work = Path(tmp)
        (work / "src").mkdir()
        for rel in ["Cargo.toml", "Cargo.lock", "src/lib.rs"]:
            shutil.copyfile(PACKAGE / rel, work / rel)
        stage(orig, ohash, work / "src/original.rs")
        staged = work / "src/generated.rs"
        stage(gen, ghash, staged)
        code, log = verify(work, out, HARNESS, kani(HARNESS))
        successful(code, log, 1)
        # This broader spike intentionally retains the actual standard-library tree.
        code, log = verify(work, out, "populated_equivalence", kani("populated_equivalence"))
        if code == 0:
            successful(code, log, 7)
            populated = "proved over the recorded domain"
        else:
            require("CBMC timed out." in log and "Failed Checks:" not in log,
                    "populated harness failed for a reason other than its budget")
            populated = "inconclusive"
        mutant_digest = mutate(staged)
        shutil.copyfile(staged, out / "guard_mutant.rs")
        code, log = verify(work, out, "guard_mutant", kani(
            MUTANT, "-Z", "concrete-playback", "--concrete-playback", "inplace"))
        require(code != 0 and re.search(r"\*\* 1 of [1-9][0-9]* failed", log) and
                f'Failed Checks: "{ASSERTION}"' in log and "undetermined" not in log and
                "VERIFICATION:- FAILED" in log, "expected the guard mutant assertion alone to fail")
        body = (work / "src/lib.rs").read_text()
        test = re.search(r"fn (kani_concrete_playback_guard_priority_counterexample_\d+)\(\)", body)
        require(test and "vec![255, 255, 255, 255, 255, 255, 255, 255]" in body,
                "Kani did not emit the signed -1 witness")
        playback_source = body[body.index("/// Test generated for harness "):]
        (out / "guard_witness.rs").write_text(playback_source)
        code, log = verify(work, out, "guard_replay", ["cargo", "kani", "playback", "-Z",
            "concrete-playback", "--", "--exact", test[1], "--nocapture"])
        require(code != 0 and "test result: FAILED. 0 passed; 1 failed" in log and
                f"test {test[1]} ... FAILED" in log and f"\n{ASSERTION}\n" in log,
                "concrete playback did not reproduce the same assertion")
    artifacts = {file: digest((out / file).read_bytes()) for file in [
        "transfer_equivalence.log", "populated_equivalence.log", "guard_mutant.log",
        "guard_replay.log", "guard_witness.rs", "guard_mutant.rs"]}
    result = dict(version=1, originalSha256=ohash, generatedSha256=ghash,
                  mutantSha256=mutant_digest, packageSha256=hashes, artifactsSha256=artifacts,
                  kani="0.68.0", cbmc="6.11.0", proofCompiler=rustc, solver="cadical",
                  flags=["-C", "overflow-checks=yes"], unwind=64, timeoutSeconds=TIMEOUT,
                  transfer_equivalence="proved over the recorded domain",
                  populated_equivalence=populated, guard_mutant="disproved", guard_replay="tested",
                  scope="empty map, source s, destination s or d, arbitrary signed i64 amount",
                  populatedScope="s/d/z rows, owners a/b/c, arbitrary admissible i64 balances, source s/x, destination s/x/d, arbitrary i64 amount",
                  unboundedKeysAndMaps="inconclusive",
                  witness=dict(rows=[], source="s", destination="d", amount=-1,
                               expected="SourceMissing", actual="NonPositiveAmount"))
    result_file.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
