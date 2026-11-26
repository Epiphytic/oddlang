#!/usr/bin/env python3
"""Compare independently derived Clap fixtures with source-free projection."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


FIXTURE = Path(__file__).resolve().parent
ODDLANG = Path(sys.argv[1]).resolve()


def execute(*argv, cwd=None, env=None):
    return subprocess.run(
        argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, capture_output=True
    )


def checked(*argv, cwd=None, env=None):
    result = execute(*argv, cwd=cwd, env=env)
    if result.returncode:
        raise RuntimeError(f"{argv} failed with {result.returncode}: {result.stderr!r}")
    return result


def observation(result):
    return result.returncode, result.stdout, result.stderr


cases = [
    [], ["--help"], ["--version"], ["--absent"], ["help", "capture"],
    ["help", "calibrate"], ["capture"], ["capture", "alpha"],
    ["scan", "--station", "coast", "--count", "10", "beta"],
    ["capture", "--count", "0", "alpha"], ["capture", "--count", "11", "alpha"],
    ["capture", "--silent", "--verbose", "gamma"], ["capture", "--export", "alpha"],
    ["capture", "--export", "--silent", "alpha"], ["calibrate"],
]

with tempfile.TemporaryDirectory(prefix="oddlang-cli-fixture-") as workspace:
    root = Path(workspace)
    checked(
        ODDLANG, "recover", "cli-surface", FIXTURE / "src/bin/reference.rs",
        FIXTURE / "selection.json", root / "graph.json",
    )
    checked(
        ODDLANG, "recover", "cli-surface", FIXTURE / "src/bin/reference.rs",
        FIXTURE / "selection.json", root / "again.json",
    )
    assert (root / "graph.json").read_bytes() == (root / "again.json").read_bytes()
    for name in ("reference", "generated"):
        target = root / name
        (target / "src").mkdir(parents=True)
        for manifest in ("Cargo.toml", "Cargo.lock"):
            shutil.copy(FIXTURE / manifest, target)
        if name == "reference":
            shutil.copy(FIXTURE / "src/bin/reference.rs", target / "src/main.rs")
        else:
            shutil.copy(FIXTURE / "generated.rs", target / "src/main.rs")
            checked(
                ODDLANG, "build", "cli-surface", "rust", root / "graph.json",
                FIXTURE / "binding.json", target / "src/cli.rs",
            )
        checked("cargo", "build", "--locked", "--offline", cwd=target)
    env = dict(os.environ, NO_COLOR="1", TERM="dumb")
    for argv in cases:
        expected = observation(execute(root / "reference/target/debug/observatory", *argv, env=env))
        actual = observation(execute(root / "generated/target/debug/observatory", *argv, env=env))
        assert actual == expected, (argv, expected, actual)

    graph = json.loads((root / "graph.json").read_bytes())
    count = graph["root"]["subcommands"][0]["arguments"][0]
    count["bounds"]["maximum"] = 9
    changed = root / "mutated.json"
    changed.write_text(json.dumps(graph))
    checked(
        ODDLANG, "build", "cli-surface", "rust", changed, FIXTURE / "binding.json",
        root / "generated/src/cli.rs",
    )
    checked("cargo", "build", "--locked", "--offline", cwd=root / "generated")
    rejected = execute(
        root / "generated/target/debug/observatory", "capture", "--count", "10", "beta",
        env=env,
    )
    assert rejected.returncode == 12 and b"1..=9" in rejected.stderr

    count["bounds"]["maximum"] = 0
    changed.write_text(json.dumps(graph))
    invalid = execute(
        ODDLANG, "build", "cli-surface", "rust", changed, FIXTURE / "binding.json",
        root / "rejected.rs",
    )
    assert invalid.returncode and not (root / "rejected.rs").exists()

    unsupported = root / "unsupported.rs"
    source = (FIXTURE / "src/bin/reference.rs").read_text()
    unsupported.write_text(source.replace("visible_alias = \"scan\"", "long_about = \"unsupported\""))
    missing = execute(
        ODDLANG, "recover", "cli-surface", unsupported, FIXTURE / "selection.json",
        root / "unsupported.json",
    )
    assert missing.returncode and not (root / "unsupported.json").exists()

print(f"{len(cases)} exact process observations, deterministic recovery, and three mutation checks passed")
