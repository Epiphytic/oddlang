"""Verify recorded release CLI observations against the independently frozen contract."""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

from observations import CONTRACT, REVISION


def verify_cli(native, directory):
    if native["revision"] != REVISION:
        raise ValueError("CLI observations describe the wrong revision")
    contract = json.loads(CONTRACT.read_bytes())["cli"]
    rows = {row["name"]: row for row in native["commands"]}
    if len(rows) != len(native["commands"]):
        raise ValueError("duplicate native command")
    release = rows.get("release")
    if not release or release["exitCode"] != 0 or release["status"] != "passed" or release["timedOut"]:
        raise ValueError("release build failed or is missing; CLI baseline unavailable")
    expected_args = ["cargo", "build", "--locked", "--offline", "--release", "-p", "marmish-native", "--bins"]
    if release["argv"] != expected_args:
        raise ValueError("unexpected release build command")

    binaries = {"marmish", "marmishd", "marmish-worker"}
    probes = {(binary, arg) for binary in binaries for arg in ("--help", "--version")}
    cases = contract["cases"]
    if len(cases) != len(probes) or {(case["binary"], case["argument"]) for case in cases} != probes:
        raise ValueError("frozen CLI contract must cover exactly six product probes")
    if set(native["binaries"]) != binaries:
        raise ValueError("incomplete release binary inventory")
    for binary, info in native["binaries"].items():
        if not re.fullmatch("[0-9a-f]{64}", info["sha256"]) or type(info["size"]) != int or info["size"] <= 0:
            raise ValueError(f"invalid release binary identity: {binary}")
    for case in cases:
        binary, argument = case["binary"], case["argument"]
        name = binary + "-" + argument.removeprefix("--")
        row = rows.get(name)
        if not row or row["argv"] != [f"$RUN/target/release/{binary}", argument]:
            raise ValueError(f"missing or incorrect CLI command: {name}")
        code = case["exitCode"]
        if row["timedOut"] or row["exitCode"] != code or row["status"] != ("passed" if code == 0 else "failed"):
            raise ValueError(f"unexpected CLI exit: {name}")
        for stream in ("stdout", "stderr"):
            relative = Path("logs") / f"{name}.{stream}"
            if row[stream] != str(relative):
                raise ValueError(f"wrong CLI log: {name} {stream}")
            data = (directory / relative).read_bytes()
            observed = {"byteLength": len(data), "sha256": hashlib.sha256(data).hexdigest()}
            if case[stream] != observed or row[stream + "Sha256"] != observed["sha256"]:
                raise ValueError(f"unexpected CLI bytes: {name} {stream}")
            if binary != "marmish-worker" and argument == "--version" and stream == "stdout":
                if data != contract["productVersionStdoutUtf8"].encode():
                    raise ValueError(f"unexpected version text: {name}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    args = parser.parse_args()
    try:
        verify_cli(json.loads(args.baseline.read_bytes()), args.baseline.parent)
    except (ValueError, OSError, KeyError, TypeError) as exc:
        sys.exit(f"invalid Marmish CLI observation: {exc}")
