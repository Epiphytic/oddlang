"""Seal a local baseline run and check its identity and recorded observations."""
import argparse
import hashlib
import json
from pathlib import Path

from cli_observations import verify_cli
from observations import CONTRACT, expand

REVISION = "49fc705408a230bd0ad896248d08dbdd2faad05a"


def sha(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def check_native(native, directory):
    if native["revision"] != REVISION or not native["commands"]:
        raise ValueError("missing pinned native observations")
    commands = {row["name"]: row for row in native["commands"]}
    required = {"ontology", "metadata", "installed-metadata", "workspace", "workspace-ignored",
                "installed", "installed-ignored", "release", "cfg", "rustc", "cargo"}
    if len(commands) != len(native["commands"]) or not required.issubset(commands):
        raise ValueError("missing or duplicate native command")
    for row in commands.values():
        for stream in ("stdout", "stderr"):
            rel = Path("logs") / (row["name"] + "." + stream)
            if row[stream] != str(rel) or sha(directory / rel) != row[stream + "Sha256"]:
                raise ValueError("native command log changed")
        if (row["exitCode"] == 0) != (row["status"] == "passed"):
            raise ValueError("native command status disagrees with exit code")


def expected_cases(contract):
    rows = []
    for case in contract["cases"]:
        value = case["expected"]
        observed = {"bytesHex": expand(value["bytes"]).hex()} if "bytes" in value else value
        rows.append({"id": case["id"], "operation": case["operation"], "observed": observed,
                     "inputSha256": hashlib.sha256(expand(case["input"])).hexdigest()})
    return rows


def collect(directory):
    names = ("inventory/inventory-summary.json", "inventory/inventory.json.gz",
             "observations/results.json", "native/baseline.json")
    for name in names:
        if not (directory / name).is_file():
            raise ValueError(f"missing baseline input: {name}")
    native = json.loads((directory / names[-1]).read_text())
    check_native(native, directory / "native")
    verify_cli(native, directory / "native")
    inventory = json.loads((directory / names[0]).read_text())
    observations = json.loads((directory / names[2]).read_text())
    if inventory["gitRevision"] != REVISION or observations["sourceRevision"] != REVISION:
        raise ValueError("component evidence describes another revision")
    contract = json.loads(CONTRACT.read_bytes())
    if observations["contractSha256"] != sha(CONTRACT) or observations["cases"] != expected_cases(contract):
        raise ValueError("framing observations differ from the frozen contract")
    if inventory["inventorySha256"] != sha(directory / names[1]):
        raise ValueError("inventory does not match its summary")
    files = {}
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"unexpected symlink in receipt: {path}")
        if path.is_file() and path != directory / "receipt.json":
            files[path.relative_to(directory).as_posix()] = sha(path)
    return {"schemaVersion": 1, "sourceRevision": REVISION, "files": files}


def check(directory, write=False):
    measured = collect(directory)
    path = directory / "receipt.json"
    if write:
        with path.open("x") as output:
            json.dump(measured, output, indent=2, sort_keys=True)
            output.write("\n")
    if json.loads(path.read_text()) != measured:
        raise ValueError("baseline evidence changed after collection")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    check(args.directory, args.write)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit(str(error))
