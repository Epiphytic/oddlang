#!/usr/bin/env python3
"""Compare independently derived Clap fixtures with source-free projection."""

import hashlib
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
    outputs = {}
    for argv in cases:
        expected = observation(execute(root / "reference/target/debug/observatory", *argv, env=env))
        actual = observation(execute(root / "generated/target/debug/observatory", *argv, env=env))
        assert actual == expected, (argv, expected, actual)
        outputs[tuple(argv)] = actual
    help_status, help_stdout, help_stderr = outputs[("--help",)]
    assert help_status == 0 and b"Usage: telescope" in help_stdout and not help_stderr
    assert outputs[("--version",)] == (0, b"observatory 2.1.0\n", b"")
    invalid_status, invalid_stdout, invalid_stderr = outputs[("--absent",)]
    assert invalid_status == 12 and not invalid_stdout
    assert invalid_stderr.endswith(b"telescope: invalid arguments\n")
    assert outputs[("calibrate",)] == (125, b"", b"command operation unavailable\n")
    assert outputs[("capture", "alpha")] == (0, b"urn:observatory:capture summit 3 alpha\n", b"")

    package_inputs = root / "package-inputs"
    package_inputs.mkdir()
    for source in (root / "graph.json", FIXTURE / "binding.json", FIXTURE / "Cargo.lock"):
        shutil.copy(source, package_inputs)
    def pin(name):
        return {"path": name, "sha256": hashlib.sha256((package_inputs / name).read_bytes()).hexdigest()}
    manifest = {
        "format": "oddlang-native-package-v1",
        "name": "observatory",
        "edition": "2021",
        "version": "2.1.0",
        "cargoLock": pin("Cargo.lock"),
        "dependencies": {"clap": {"version": "4.6.7", "features": ["derive"]}},
        "components": [{
            "kind": "cli-surface", "module": "command",
            "graph": pin("graph.json"), "overlay": pin("binding.json"),
            "reexports": ["run_cli", "run_cli_with"],
        }],
    }
    package_manifest = package_inputs / "package.json"
    package_manifest.write_text(json.dumps(manifest))
    for name in ("package-first", "package-second"):
        checked(ODDLANG, "build", "native-package", "rust", package_manifest, root / name)
        checked("cargo", "check", "--locked", "--offline", cwd=root / name)
    package_lock = (root / "package-first/package.lock").read_bytes()
    assert package_lock == (root / "package-second/package.lock").read_bytes()
    assert b"oddlang-command-surface-lock-v1" in package_lock
    manifest["dependencies"]["clap"]["version"] = "4.6.6"
    package_manifest.write_text(json.dumps(manifest))
    wrong_provider = execute(
        ODDLANG, "build", "native-package", "rust", package_manifest, root / "package-invalid",
    )
    assert wrong_provider.returncode and b"pinned clap 4.6.7 provider" in wrong_provider.stderr

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

    count["bounds"]["maximum"] = 10
    count["default_value"] = "not-an-integer"
    changed.write_text(json.dumps(graph))
    invalid = execute(
        ODDLANG, "build", "cli-surface", "rust", changed, FIXTURE / "binding.json",
        root / "rejected.rs",
    )
    assert invalid.returncode and not (root / "rejected.rs").exists()
    count["default_value"] = "3"

    args = graph["root"]["subcommands"][0]["arguments"]
    args[-1]["repeated"] = True
    args.append({"id": "tail", "kind": "string", "value_name": "TAIL"})
    changed.write_text(json.dumps(graph))
    invalid = execute(
        ODDLANG, "build", "cli-surface", "rust", changed, FIXTURE / "binding.json",
        root / "rejected.rs",
    )
    assert invalid.returncode and b"ambiguous placement" in invalid.stderr

    unsupported = root / "unsupported.rs"
    source = (FIXTURE / "src/bin/reference.rs").read_text()
    unsupported.write_text(source.replace("visible_alias = \"scan\"", "long_about = \"unsupported\""))
    missing = execute(
        ODDLANG, "recover", "cli-surface", unsupported, FIXTURE / "selection.json",
        root / "unsupported.json",
    )
    assert missing.returncode and not (root / "unsupported.json").exists()

    unsupported.write_text(source.replace("    sensor: OsString,", """    sensor: OsString,
    #[arg(long, value_enum)]
    filter: Option<Filter>,""") + """
#[derive(clap::ValueEnum, Clone)]
enum Filter {
    /// Use the red filter.
    Red,
}""")
    missing = execute(
        ODDLANG, "recover", "cli-surface", unsupported, FIXTURE / "selection.json",
        root / "unsupported.json",
    )
    assert missing.returncode and b"unsupported value variant Red" in missing.stderr

print(f"{len(cases)} exact process observations, deterministic recovery and assembly, and seven mutation checks passed")
