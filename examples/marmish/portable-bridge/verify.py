#!/usr/bin/env python3
"""Replay the pinned bridge CLI slice through an external executable."""

import argparse
import fcntl
import hashlib
import json
import os
import re
import resource
import selectors
import stat
import sys
import subprocess
import tempfile
import time
from pathlib import Path

TIMEOUT = 10
MAX_REPLY = 131072
ENVIRONMENT = {"PATH": "/usr/bin:/bin", "LANG": "C"}
UMASK = 0o077
NOFILE = 256
FIXTURE = Path(__file__).with_name("baseline.json")


def compare(label, expected, observed):
    if expected != observed:
        raise ValueError(f"{label}: expected {expected!r}, observed {observed!r}")


def compare_process(case, expected, observed):
    for field in ("status", "stdout_after_eof", "stderr"):
        compare(f"{case}: {field}", expected[field], observed[field])


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def read_reply(pipe):
    deadline = time.monotonic() + TIMEOUT
    raw = bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(pipe, selectors.EVENT_READ)
        while not raw.endswith(b"\n"):
            remaining = deadline - time.monotonic()
            if len(raw) >= MAX_REPLY or remaining <= 0 or not selector.select(remaining):
                raise ValueError("no bounded reply before stdin EOF")
            chunk = os.read(pipe.fileno(), MAX_REPLY - len(raw))
            if not chunk:
                raise ValueError("stdout closed before the complete reply")
            raw.extend(chunk)
    return raw.decode("latin1")


def inspect_state(root):
    entries = {}
    hashes = {}
    for path in sorted(root.rglob("*")):
        name = path.relative_to(root).as_posix()
        metadata = path.lstat()
        compare(f"{name}: owner", os.geteuid(), metadata.st_uid)
        mode = f"{stat.S_IMODE(metadata.st_mode):04o}"
        if stat.S_ISDIR(metadata.st_mode):
            entries[name] = {"type": "dir", "mode": mode}
        elif stat.S_ISREG(metadata.st_mode):
            entries[name] = {"type": "file", "mode": mode, "bytes": metadata.st_size}
            hashes[name] = sha256(path)
        else:
            raise ValueError(f"unexpected filesystem entry {name!r}")
        if name == "state/engine.key":
            with path.open("rb") as key:
                entries[name]["magic"] = key.read(8).decode("latin1")
    lock = "absent"
    if "state/frontend.lock" in entries:
        # Linux OFD locks conflict with POSIX byte-range locks. BSD flock has a
        # separate lock space there. Darwin's original uses the BSD primitive.
        linux = sys.platform.startswith("linux")
        probe = fcntl.lockf if linux else fcntl.flock
        with (root / "state/frontend.lock").open("r+b" if linux else "rb") as descriptor:
            try:
                probe(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                lock = "held"
            else:
                lock = "available"
                probe(descriptor, fcntl.LOCK_UN)
    return {"entries": entries, "frontend_lock": lock}, hashes


def compare_state(case, expected, observed):
    compare(f"{case}: frontend lock", expected["frontend_lock"], observed["frontend_lock"])
    compare(f"{case}: entries", set(expected["entries"]), set(observed["entries"]))
    for name, (kind, mode, *contents) in expected["entries"].items():
        entry = observed["entries"][name]
        label = f"{case}: {name}"
        compare(f"{label}: type", kind, entry["type"])
        compare(f"{label}: mode", mode, entry["mode"])
        if contents == ["key40"]:
            compare(f"{label}: length", 40, entry["bytes"])
            compare(f"{label}: magic", "MRMSKEY1", entry["magic"])
        elif contents == ["empty"]:
            compare(f"{label}: length", 0, entry["bytes"])
        elif contents == ["nonempty"]:
            compare(f"{label}: nonempty", True, entry["bytes"] > 0)


def constrain_child():
    resource.setrlimit(resource.RLIMIT_NOFILE, (NOFILE, NOFILE))


def child_limits():
    limits = {
        name.removeprefix("RLIMIT_"): resource.getrlimit(getattr(resource, name))
        for name in sorted(dir(resource))
        if name.startswith("RLIMIT_") and name != "RLIMIT_INFINITY"
    }
    limits["NOFILE"] = (NOFILE, NOFILE)
    return limits


def run_case(baseline, case, bridge, worker):
    name = case["id"]
    with tempfile.TemporaryDirectory(prefix="oddlang-bridge-", dir="/tmp") as temporary:
        root = Path(temporary).resolve()
        cwd = root / "cwd"
        cwd.mkdir(mode=0o700)
        cwd.chmod(0o700)
        path_fields = {
            "STATE": json.dumps(str(root / "state")),
            "WORKER": json.dumps(str(worker)),
            "CWD": json.dumps(str(cwd)),
        }
        initial, last_hashes = inspect_state(root)
        compare_state(name, baseline["states"]["absent"], initial)
        exchanges = []
        with subprocess.Popen(
            [bridge, *case["args"]],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            env=ENVIRONMENT,
            umask=UMASK,
            preexec_fn=constrain_child,
            bufsize=0,
        ) as child:
            try:
                for exchange in case["exchanges"]:
                    fields = dict(path_fields, VERSION=str(exchange["version"]))
                    request = re.sub(r"@([A-Z]+)@", lambda match: fields[match[1]], baseline["request"])
                    wire = request.encode("ascii")
                    compare(f"{name}: request write", len(wire), child.stdin.write(wire))
                    reply = read_reply(child.stdout)
                    compare(f"{name}: stdout before EOF", baseline["replies"][exchange["reply"]], reply)
                    state, hashes = inspect_state(root)
                    compare_state(name, baseline["states"][exchange["state"]], state)
                    observation = {"stdin": request, "stdout": reply, "state": state}
                    if exchange.get("unchanged"):
                        unchanged = last_hashes == hashes
                        compare(f"{name}: content unchanged", True, unchanged)
                        observation["contents_unchanged"] = unchanged
                    exchanges.append(observation)
                    last_hashes = hashes
                tail = case["stdin_before_eof"]
                stdout, stderr = child.communicate(tail.encode("latin1"), timeout=TIMEOUT)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=TIMEOUT)
        final, hashes = inspect_state(root)
        compare_state(name, baseline["states"][case["final_state"]], final)
        key = "state/engine.key"
        retained_key = None
        if key in last_hashes:
            retained_key = hashes[key] == last_hashes[key]
            compare(f"{name}: key retained after EOF", True, retained_key)
        result = {
            "id": name,
            "args": case["args"],
            "cwd": str(cwd),
            "initial_state": initial,
            "exchanges": exchanges,
            "stdin_before_eof": tail,
            "stdout_after_eof": stdout.decode("latin1"),
            "stderr": stderr.decode("latin1"),
            "status": child.returncode,
            "final_state": final,
            "key_retained_after_eof": retained_key,
        }
        compare_process(name, case, result)
        return result


def reject(case, expected, observed, channel):
    try:
        compare_process(case, expected, observed)
    except ValueError as error:
        if str(error).startswith(f"{case}: {channel}:"):
            return "rejected"
        raise
    raise ValueError(f"{case}: {channel} mutation passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bridge", type=Path)
    parser.add_argument("worker", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if os.geteuid() == 0:
        parser.error("the pinned original requires an unprivileged account")
    bridge = args.bridge.resolve(strict=True)
    worker = args.worker.resolve(strict=True)
    baseline = json.loads(FIXTURE.read_bytes())
    cases = [run_case(baseline, case, bridge, worker) for case in baseline["cases"]]
    by_id = {case["id"]: case for case in cases}
    fixture_cases = {case["id"]: case for case in baseline["cases"]}
    original = fixture_cases["argument-rejection"]
    wrong_status = dict(original, status=0)
    changed_status = reject(original["id"], wrong_status, by_id[original["id"]], "status")
    partial = fixture_cases["partial-frame-at-eof"]
    wrong_channel = dict(partial, stderr=partial["stdout_after_eof"], stdout_after_eof="")
    changed_channel = reject(partial["id"], wrong_channel, by_id[partial["id"]], "stdout_after_eof")
    result = {
        "format": 1,
        "fixture_sha256": sha256(FIXTURE),
        "binary_sha256": {"bridge": sha256(bridge), "worker": sha256(worker)},
        "environment": ENVIRONMENT,
        "umask": f"{UMASK:04o}",
        "startup_limits": child_limits(),
        "cases": cases,
        "controls": {"expected_status": changed_status, "wrong_channel": changed_channel},
    }
    if args.report:
        args.report.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"{len(cases)} bridge CLI cases passed. Changed status and output channel rejected.")


if __name__ == "__main__":
    main()
