"""Compare a local committed candidate with the immutable native Marmish reference."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import platform
import re
import stat
import subprocess
import sys
import tempfile
import tarfile
import tomllib

import baseline
import original

ROOT = Path(__file__).resolve().parents[2]
FRAME = "crates/marmish-core/src/frame.rs"
ONTOLOGY = {f"docs/ontology/{n}" for n in ("source_files.jsonl", "symbols.jsonl", "code_references.jsonl")}


def require(test, message):
    if not test:
        raise ValueError(message)


def save(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def git(directory, *args):
    return subprocess.check_output(["git", "-C", directory, *args])


def trees(repo):
    rows = {}
    for line in git(repo, "ls-tree", "-r", "-z", "HEAD").split(b"\0"):
        if line:
            metadata, name = line.split(b"\t", 1)
            rows[name.decode()] = metadata
    return rows


def identities(upstream, candidate):
    require(git(upstream, "rev-parse", "HEAD").decode().strip() == baseline.PIN, "wrong original revision")
    require(git(upstream, "rev-parse", "HEAD^{tree}").decode().strip() == original.TREE, "wrong original tree")
    revision = git(candidate, "rev-parse", "HEAD").decode().strip()
    require(re.fullmatch("[a-f0-9]{40}", revision) and revision != baseline.PIN, "expected distinct candidate commit")
    before, after = trees(upstream), trees(candidate)
    require(before.keys() == after.keys(), "candidate changed tracked paths")
    changed = {path for path in before if before[path] != after[path]}
    evidence = candidate.parent / "first"
    manifest = evidence / "project.toml"
    pin = (ROOT / "examples/marmish/assembly-v2.sha256").read_text().split()[0]
    require(baseline.sha(manifest) == pin, "candidate assembly manifest changed")
    generated = {r["path"]: r for r in tomllib.loads(manifest.read_text())["files"] if r["producer"] == "generated"}
    require(set(generated) == {FRAME, "crates/marmish-core/src/lib.rs"} and
            set(generated) <= changed <= ONTOLOGY | set(generated), "unexpected candidate source change")
    for path, row in generated.items():
        body = git(candidate, "show", "HEAD:" + path)
        folder = "framing-projection" if path == FRAME else "facade-projection"
        projected = evidence / folder / row["module"]
        require(hashlib.sha256(body).hexdigest() == row["sha256"] == baseline.sha(projected),
                f"candidate module differs from locked producer: {path}")
    require(all(before[p].split()[:2] == after[p].split()[:2] for p in changed), "changed candidate modes")
    return revision


def snapshot(base):
    files = {}
    for path in sorted(base.rglob("*")):
        mode = path.lstat().st_mode
        require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), "unexpected fixture file type")
        files[path.relative_to(base).as_posix()] = {"mode": oct(mode),
                "sha256": baseline.sha(path) if stat.S_ISREG(mode) else None}
    return files


def expected_status(prefix, bin_dir):
    return {"schema": 1, "operation": "status", "version": None, "target": None, "active": False,
            "generation": 0, "prefix": str(prefix), "bin_dir": str(bin_dir)}


def installed_status(tmp, policy, env, report, state):
    # Seed the documented inactive receipt directly. On a virgin prefix the
    # product otherwise initializes that receipt during the first status query.
    base = tmp / "installed-layout"
    prefix, bin_dir, state_dir = (base / name for name in ("prefix", "bin", "state"))
    for folder in (prefix, bin_dir, state_dir):
        folder.mkdir(mode=0o700, parents=True)
    receipt = prefix / ".marmish-native-install.json"
    record = {"format": 1, "install_id": "0123456789abcdef0123456789abcdef", "prefix": str(prefix),
              "bin_dir": str(bin_dir), "generation": 0, "active": None, "launchers": []}
    receipt.write_text(json.dumps(record, separators=(",", ":")) + "\n")
    receipt.chmod(0o600)
    (prefix / ".lock").touch(mode=0o600)
    initial = snapshot(base)
    args = [tmp / "target/release/marmish", "install-status", "--prefix", prefix,
            "--bin-dir", bin_dir, "--state-dir", state_dir]
    result = subprocess.run([*policy, *map(str, args)], env=env, cwd=tmp / "source",
                            stdin=subprocess.DEVNULL, capture_output=True, timeout=30)
    (report / "install-status.stdout").write_bytes(result.stdout)
    (report / "install-status.stderr").write_bytes(result.stderr)
    expected = expected_status(prefix, bin_dir)
    unchanged = snapshot(base) == initial
    byte_match = result.stdout == (json.dumps(expected, separators=(",", ":")) + "\n").encode()
    builds = list((tmp / "target/release/build").glob("marmish-native-*/output"))
    require(len(builds) == 1, "release build provenance unavailable")
    commit = f"cargo:rustc-env=MARMISH_BUILD_COMMIT={state['revision']}"
    target = f"cargo:rustc-env=MARMISH_BUILD_TARGET={baseline.TARGET}"
    lines = builds[0].read_text().splitlines()
    require(commit in lines and target in lines, "release binary compiled for unexpected source")
    (report / "build-metadata.txt").write_bytes(builds[0].read_bytes())
    state["sourceBuild"] = {"commit": state["revision"], "target": baseline.TARGET,
                            "metadataSha256": baseline.sha(builds[0])}
    state["installedStatus"] = {"scenario": "inactive pre-seeded layout", "exitCode": result.returncode,
                                "stdoutMatchesExpected": byte_match, "stderrEmpty": not result.stderr,
                                "fixtureUnchanged": unchanged, "fixtureBefore": initial,
                                "fixtureAfter": snapshot(base)}


def collect(upstream, candidate, directory):
    require((platform.system(), platform.machine()) == ("Darwin", "arm64"), "native Apple Silicon required")
    revision = identities(upstream, candidate)
    directory.mkdir(parents=True, exist_ok=True)
    require(not any(directory.iterdir()), "qualification requires an empty output directory")
    original.recover(directory / "original")
    native = directory / "candidate"
    native.mkdir()
    with tempfile.TemporaryDirectory(prefix="m-", dir="/private/tmp") as tmp:
        baseline.run(candidate, native, Path(tmp), revision=revision, after_release=installed_status)
    return revision


SUMMARY = re.compile(r'^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                     r'\d+ measured; 0 filtered out;.*$', re.M)
TEST = re.compile(r'^test (.+?) \.\.\. (ok|FAILED|ignored(?:, .*)?|)$')


def test_outcomes(stdout, stderr):
    targets = []
    for line in stderr.splitlines():
        proc = re.match(r'^\s*Running .+? \((.+)\)$', line)
        docs = re.match(r'^\s*Doc-tests (\S+)$', line)
        if proc:
            targets.append(re.sub(r'-[0-9a-f]{16}$', '', Path(proc[1]).name))
        elif docs:
            targets.append("doc/" + docs[1])
    summaries = list(SUMMARY.finditer(stdout))
    require(len(summaries) == len(targets) and summaries, "incomplete libtest run or missing target identity")
    all_rows = {}
    start = 0
    for target, summary in zip(targets, summaries, strict=True):
        rows = {}
        pending = None
        for line in stdout[start:summary.start()].splitlines():
            match = TEST.fullmatch(line)
            if match:
                name, status = match.groups()
                if not status:
                    pending = name
                else:
                    rows[name] = status
            elif line in ("ok", "FAILED") and pending:
                rows[pending] = line
                pending = None
        counts = Counter(v.split(", ", 1)[0] for v in rows.values())
        require([counts[n] for n in ("ok", "FAILED", "ignored")] == [int(s) for s in summary.groups()],
                f"individual outcomes disagree with libtest summary for {target}")
        for name, outcome in rows.items():
            key = target + "/" + name
            require(key not in all_rows, f"duplicate test identity: {key}")
            all_rows[key] = outcome
        start = summary.end()
    return all_rows


def deltas(originals, candidates):
    return [{"test": name, "original": originals.get(name, "unobserved"),
             "candidate": candidates.get(name, "unobserved")}
            for name in sorted(originals.keys() | candidates.keys()) if originals.get(name) != candidates.get(name)]


def command_rows(directory, metadata):
    rows = {}
    for row in metadata["commands"]:
        name = row["name"]
        require(name not in rows and re.fullmatch("[a-z0-9-]+", name), "invalid native command name")
        for stream in ("stdout", "stderr"):
            path = Path("logs") / (name + "." + stream)
            require(row[stream] == str(path) and baseline.sha(directory / path) == row[stream + "Sha256"],
                    f"native stream digest mismatch: {name}.{stream}")
        status = "timed-out" if row["timedOut"] else "failed" if row["exitCode"] else "passed"
        require(row["status"] == status, f"invalid native status: {name}")
        rows[name] = row
    return rows


def improvements(command_differences, tests):
    fixed = [{"kind": "cargo-exit", **row} for row in command_differences
             if row["command"] in ("workspace", "installed") and row["original"] != 0 and row["candidate"] == 0]
    for suite, result in tests.items():
        for row in result["differences"]:
            if row["candidate"] == "ok" and (row["original"] == "FAILED" or row["original"].startswith("ignored")):
                fixed.append({"kind": "test", "suite": suite, **row})
    return fixed


def sdk_diagnostics(data):
    # xcrun uses mkstemp names when it reports denied writes to its host cache.
    # Compare the exact diagnostic, errno, directory and multiplicity.
    return re.sub(rb"(?m)^(xcrun: error: couldn't create cache file '/var/folders/"
                  rb"[^/\n']+/[^/\n']+/T/xcrun_db-)[A-Za-z0-9]{8}"
                  rb"(' \(errno=Operation not permitted\))$", rb"\1<RANDOM>\2", data)


def listed_ignored(path):
    return sorted(line.removesuffix(": test") for line in path.read_text().splitlines() if line.endswith(": test"))


def cargo_metadata(path):
    # Paths into independently cloned source trees and empty target directories vary per run.
    text = re.sub(r"/private/tmp/m-[^/\\\s\"]+/", "$RUN/", path.read_text())
    return json.loads(text)


def failure_locations(log_path, test):
    needle = f"---- {test.split('/', 1)[1]} stdout ----"
    return [i for i, line in enumerate(log_path.read_text().splitlines(), start=1) if line == needle]


def compare_archived(directory, original_commit, candidate_commit, candidate_tree):
    """Recompute the comparison solely from authenticated reference and recorded native evidence."""
    revision = candidate_commit
    require(original_commit == baseline.PIN, "wrong archived original identity")
    require(re.fullmatch("[0-9a-f]{40}", revision) and revision != baseline.PIN, "wrong archived candidate identity")
    ori, cand = directory / "original/native", directory / "candidate"
    require(baseline.sha(original.ARCHIVE) == original.DIGEST, "published original archive changed")
    with tarfile.open(original.ARCHIVE, "r:gz") as archive:
        require((ori.parent / "receipt.json").read_bytes() == archive.extractfile("receipt.json").read(),
                "recovered original receipt changed")
    original.check(ori.parent)
    a, b = [json.loads((d / "baseline.json").read_bytes()) for d in (ori, cand)]
    require(a["revision"] == baseline.PIN and b["revision"] == revision, "unexpected test provenance")
    require(a["tree"] == original.TREE and b["tree"] == candidate_tree and re.fullmatch("[0-9a-f]{40}", candidate_tree),
            "archived Git tree differs")
    inputs = command_rows(ori, a), command_rows(cand, b)
    original_rows, candidate_rows = inputs
    require(len(original_rows) == 22, "incomplete reference matrix")
    regressions = []
    command_differences = []
    allowed_variations = []
    for name in sorted(set(original_rows) | set(candidate_rows)):
        left, right = original_rows.get(name), candidate_rows.get(name)
        if not left or not right:
            regressions.append(f"command membership changed: {name}")
            continue
        if left["argv"] != right["argv"]:
            regressions.append(f"command arguments changed: {name}")
        if left["status"] != right["status"] or left["exitCode"] != right["exitCode"]:
            command_differences.append({"command": name, "original": left["exitCode"], "candidate": right["exitCode"]})
            if name not in ("installed", "workspace") or right["exitCode"] != 0:
                regressions.append(f"command outcome changed: {name}")
        if right["timedOut"]:
            regressions.append(f"timeout: {name}")
        if (name in ("cfg", "cargo", "clang", "git", "python", "rustc", "sdk", "uv") or
                name.endswith("-help") or name.endswith("-version")):
            streams_match = left["stderrSha256"] == right["stderrSha256"]
            if name == "sdk" and not streams_match:
                streams_match = sdk_diagnostics((ori/"logs/sdk.stderr").read_bytes()) == sdk_diagnostics(
                    (cand/"logs/sdk.stderr").read_bytes())
                if streams_match:
                    allowed_variations.append({"command": name, "stream": "stderr", "invariant":
                        "Only the generated eight-character suffix of a denied xcrun cache file varies.",
                        "originalSha256": left["stderrSha256"], "candidateSha256": right["stderrSha256"]})
            if left["stdoutSha256"] != right["stdoutSha256"] or not streams_match or left["exitCode"] != right["exitCode"]:
                regressions.append(f"public output or tool identity changed: {name}")
    if a["isolation"]["profileSha256"] != b["isolation"]["profileSha256"] or (
            baseline.sha(cand / "baseline-macos.sb") != b["isolation"]["profileSha256"]):
        regressions.append("native sandbox profile changed")
    for key in ("host", "tools", "locks", "flags", "platforms"):
        if a[key] != b[key]:
            regressions.append("native configuration changed: " + key)
    for mode in ("metadata", "installed-metadata"):
        if cargo_metadata(ori / "logs" / (mode + ".stdout")) != cargo_metadata(cand / "logs" / (mode + ".stdout")):
            regressions.append(f"resolved Cargo metadata changed: {mode}")
    if b.get("sourceBuild", {}).get("commit") != revision or not (cand / "build-metadata.txt").is_file():
        regressions.append("candidate release build metadata missing")
    elif (b["sourceBuild"]["metadataSha256"] != baseline.sha(cand / "build-metadata.txt") or
          f"cargo:rustc-env=MARMISH_BUILD_COMMIT={revision}" not in (cand / "build-metadata.txt").read_text().splitlines() or
          f"cargo:rustc-env=MARMISH_BUILD_TARGET={baseline.TARGET}" not in (cand / "build-metadata.txt").read_text().splitlines()):
        regressions.append("candidate release build metadata changed")
    tests, cohorts = {}, defaultdict(dict)
    for mode in ("workspace", "installed"):
        both = [test_outcomes((d/"logs"/(mode+".stdout")).read_text(), (d/"logs"/(mode+".stderr")).read_text())
                for d in (ori, cand)]
        differences = deltas(*both)
        for diff in differences:
            diff["transcripts"] = {}
            for side, folder in (("original", ori), ("candidate", cand)):
                path = folder / "logs" / (mode + ".stdout")
                diff["transcripts"][side] = {"path": str(path.relative_to(directory)),
                                            "failureLines": failure_locations(path, diff["test"])}
            if diff["candidate"] != "ok" or diff["original"] == "unobserved":
                regressions.append(f"{mode}: {diff['test']} ({diff['original']} -> {diff['candidate']})")
        ignores = [listed_ignored(d/"logs"/(mode+"-ignored.stdout")) for d in (ori, cand)]
        if ignores[0] != ignores[1]:
            regressions.append(f"ignored test list changed: {mode}")
        tests[mode] = {"original": both[0], "candidate": both[1], "differences": differences,
                       "ignoredLists": dict(zip(("original", "candidate"), ignores, strict=True)),
                       "counts": {label: dict(Counter(v.split(", ", 1)[0] for v in result.values()))
                                  for label, result in zip(("original", "candidate"), both, strict=True)}}
        if mode == "installed":
            for label, outcomes in zip(("original", "candidate"), both, strict=True):
                for name, result in outcomes.items():
                    if "::installed_check::" in name:
                        cohort = name.split("::installed_check::", 1)[1].split("::")[0]
                        cohorts[cohort].setdefault(label, {})[name] = result
    cli = {}
    for binary in baseline.BINS:
        for option in ("version", "help"):
            name = f"{binary}-{option}"
            cli[name] = {label: {key: row[key] for key in ("argv", "exitCode", "stdoutSha256", "stderrSha256")}
                         for label, rows in (("original", original_rows), ("candidate", candidate_rows))
                         if (row := rows.get(name))}
    binaries = {name: {"original": a["binaries"][name], "candidate": b.get("binaries", {}).get(name)}
                for name in baseline.BINS}
    for name, pair in binaries.items():
        value = pair["candidate"]
        if not value or not re.fullmatch("[a-f0-9]{64}", value["sha256"]) or value["size"] <= 0:
            regressions.append(f"release binary unobserved: {name}")
    probe = b.get("installedStatus", {})
    if (cand / "install-status.stdout").is_file() and (cand / "install-status.stderr").is_file():
        raw = (cand / "install-status.stdout").read_bytes()
        try:
            observed = json.loads(raw)
            prefix, bin_dir = Path(observed["prefix"]), Path(observed["bin_dir"])
            require(prefix.name == "prefix" and bin_dir.name == "bin" and
                    prefix.parent == bin_dir.parent and prefix.parent.name == "installed-layout",
                    "wrong status fixture paths")
            exact = (json.dumps(expected_status(prefix, bin_dir), separators=(",", ":")) + "\n").encode()
            require(raw == exact and not (cand / "install-status.stderr").read_bytes(),
                    "wrong installed status output")
            fixture = probe.get("fixtureBefore", {})
            require(set(fixture) == {"prefix", "bin", "state", "prefix/.lock",
                                     "prefix/.marmish-native-install.json"}, "incomplete layout snapshot")
            receipt = {"format": 1, "install_id": "0123456789abcdef0123456789abcdef",
                       "prefix": str(prefix), "bin_dir": str(bin_dir), "generation": 0,
                       "active": None, "launchers": []}
            hashes = {"prefix/.lock": hashlib.sha256(b"").hexdigest(),
                      "prefix/.marmish-native-install.json": hashlib.sha256(
                          (json.dumps(receipt, separators=(",", ":")) + "\n").encode()).hexdigest()}
            for path, recorded in fixture.items():
                if isinstance(recorded, dict):
                    mode, digest = stat.S_IMODE(int(recorded["mode"], 8)), recorded["sha256"]
                else:
                    mode, digest = recorded
                require((mode, digest) == (0o600 if path in hashes else 0o700, hashes.get(path)),
                        "fixture state differs from seeded inactive layout")
        except (ValueError, KeyError, TypeError):
            regressions.append("installed layout output differs from expected bytes")
    else:
        regressions.append("installed layout transcript missing")
    if (probe.get("exitCode") != 0 or not all(probe.get(k) for k in
            ("stdoutMatchesExpected", "stderrEmpty", "fixtureUnchanged")) or
            probe.get("fixtureBefore") != probe.get("fixtureAfter")):
        regressions.append("installed layout observation failed")
    failures = sorted({name for mode in tests.values() for name, value in mode["candidate"].items() if value == "FAILED"})
    status = "regressed" if regressions else "baseline failures remain" if failures else "passed measured observations"
    result = {"schemaVersion": 1, "status": status, "originalCommit": a["revision"], "originalTree": a["tree"],
              "candidateCommit": revision, "candidateTree": b["tree"], "host": b["host"],
              "platforms": b["platforms"], "nativeCommands": len(candidate_rows), "binaries": binaries,
              "runnerSha256": b["runnerSha256"], "verifierSha256": baseline.sha(Path(__file__)),
              "originalArchiveSha256": original.DIGEST,
              "binaryComparison": "Source builds include distinct commit metadata. Each digest identifies its own artifact.",
              "sourceBuild": b.get("sourceBuild"), "installStatus": probe, "tests": tests, "cli": cli,
              "historical": json.loads((ROOT / "examples/marmish/observations.json").read_bytes())["historical"],
              "isolatedInstalledDriverUnitTests": dict(cohorts), "currentFailures": failures,
              "commandDifferences": command_differences, "regressions": regressions,
              "allowedVariations": allowed_variations,
              "improvements": improvements(command_differences, tests),
              "unobserved": ["release artifacts and installed multi-host cohorts", "native Linux x86-64 and ARM64",
                             "interactive Keychain and Tor", "remote dispatch"]}
    return result


def compare(upstream, candidate, directory):
    revision = identities(upstream, candidate)
    tree = git(candidate, "rev-parse", "HEAD^{tree}").decode().strip()
    result = compare_archived(directory, baseline.PIN, revision, tree)
    save(directory / "qualification.json", result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--compare-existing", action="store_true")
    args = parser.parse_args()
    try:
        upstream, candidate, directory = (p.resolve() for p in (args.original, args.candidate, args.output))
        if not args.compare_existing:
            collect(upstream, candidate, directory)
        result = compare(upstream, candidate, directory)
        print(f"{result['status']}: {directory / 'qualification.json'}")
        if result["regressions"]:
            sys.exit(3)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        sys.exit(f"candidate qualification failed: {error}")
