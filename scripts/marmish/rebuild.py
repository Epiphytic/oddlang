"""Recover two locked modules and independently assemble and index the candidate twice."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

from prepare import ROOT, digest
from prepare_v2 import compose

PIN = "49fc705408a230bd0ad896248d08dbdd2faad05a"
ONTOLOGY = {
    f"docs/ontology/{name}.jsonl"
    for name in ("code_references", "source_files", "symbols")
}
OFFLINE = ROOT / "scripts/marmish/offline.sh"


def output(args, cwd, env=None):
    return subprocess.check_output(args, cwd=cwd, env=env)


def identities(raw):
    entries = {}
    for row in raw.rstrip(b"\0").split(b"\0"):
        info, path = row.split(b"\t", 1)
        first, second, third = info.decode().split()
        # ls-tree returns mode, type, object. ls-files returns mode, object, stage.
        entries[path.decode()] = (first, third if second == "blob" else second)
    return entries


def run_build(upstream, destination, evidence, binary, original, env):
    evidence.mkdir()
    manifest = upstream / "crates/marmish-core/Cargo.toml"
    with tempfile.TemporaryDirectory(prefix="oddlang-recover-") as staging:
        stage = Path(staging)

        def cli(*args, hide_source=False):
            command = [binary, *args]
            if hide_source:
                command = [
                    "sandbox-exec",
                    "-D",
                    f"ORIGINAL={upstream}",
                    "-p",
                    '(version 1)(allow default)(deny network-outbound (remote ip "*:*"))(deny network-inbound (local ip "*:*"))(deny file-read* (subpath (param "ORIGINAL")))',
                    *command,
                ]
            subprocess.run(
                [OFFLINE, *command] if not hide_source else command,
                cwd=stage,
                check=True,
                env=env,
            )

        for name in ("framing", "facade"):
            mapping = ROOT / "examples/marmish" / f"{name}.toml"
            imported, projected = stage / name, stage / f"{name}-projection"
            release = json.loads(
                (ROOT / "examples/marmish" / f"{name}-release.json").read_bytes()
            )
            cli("import", "rust", manifest, mapping, imported)
            cli("import", "replay", manifest, mapping, imported)
            cli(
                "import",
                "verify",
                imported,
                "--import-sha256",
                release["importSha256"],
                hide_source=True,
            )
            cli(
                "build",
                "module",
                imported,
                projected,
                "--import-sha256",
                release["importSha256"],
                hide_source=True,
            )
            shutil.copytree(imported, evidence / name)
            shutil.copytree(projected, evidence / f"{name}-projection")
        prepared = stage / "prepared"
        compose(
            upstream,
            stage / "framing",
            stage / "framing-projection",
            stage / "facade",
            stage / "facade-projection",
            prepared,
        )
        assembly = prepared / "project.toml"
        assembly_hash = digest(assembly.read_bytes())
        if (
            assembly_hash
            != (ROOT / "examples/marmish/assembly-v2.sha256").read_text().split()[0]
        ):
            raise ValueError(
                "workspace plan differs from the approved multi-producer manifest"
            )
        cli("assemble", assembly, prepared / "store", destination, hide_source=True)
        shutil.copyfile(assembly, evidence / "project.toml")
    git_env = dict(
        env,
        GIT_CONFIG_NOSYSTEM="1",
        GIT_CONFIG_GLOBAL="/dev/null",
        GIT_AUTHOR_NAME="Oddlang qualification",
        GIT_COMMITTER_NAME="Oddlang qualification",
        GIT_AUTHOR_EMAIL="qualification@oddlang.invalid",
        GIT_COMMITTER_EMAIL="qualification@oddlang.invalid",
    )
    date = output(["git", "show", "-s", "--format=%aI", PIN], upstream).decode().strip()
    git_env.update(GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date)

    def git(*args):
        return output(
            [
                "git",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgSign=false",
                *args,
            ],
            destination,
            git_env,
        )

    git("init", "-q", "--initial-branch=candidate", "--template=")
    tags = (ROOT / "examples/marmish/ontology-tags.lock").read_text().splitlines()
    for line in tags:
        object_id, ref = line.split()
        if output(["git", "rev-parse", ref], upstream).decode().strip() != object_id:
            raise ValueError(f"upstream ontology tag changed: {ref}")
        git(
            "fetch",
            "-q",
            "--no-tags",
            "--no-write-fetch-head",
            str(upstream),
            f"{ref}:{ref}",
        )
    git("add", "-f", "--all")
    before = identities(git("ls-files", "-s", "-z"))
    rows = tomllib.loads((evidence / "project.toml").read_text())["files"]
    if original.keys() != before.keys() or {r["path"] for r in rows} != before.keys():
        raise ValueError("assembled file set differs from pinned Git tree")
    generated = {}
    for row in rows:
        path = row["path"]
        if (
            digest((destination / path).read_bytes()) != row["sha256"]
            or before[path][0] != row["mode"]
        ):
            raise ValueError(f"assembled bytes/mode disagree with manifest: {path}")
        if row["producer"] == "generated":
            if original[path] == before[path]:
                raise ValueError(f"expected distinct native projection: {path}")
            generated[path] = {
                "originalBlob": original[path][1],
                "candidateBlob": before[path][1],
                "sha256": row["sha256"],
            }
        elif original[path] != before[path]:
            raise ValueError(f"retained Git entry changed: {path}")
    before_tree = git("write-tree").decode().strip()
    with tempfile.TemporaryDirectory(prefix="oddlang-index-venv-") as temp:
        index_env = dict(
            git_env,
            UV_PROJECT_ENVIRONMENT=str(Path(temp) / "venv"),
            UV_OFFLINE="1",
            UV_PYTHON_DOWNLOADS="never",
        )

        def index(*args):
            return output(
                [
                    OFFLINE,
                    "uv",
                    "run",
                    "--locked",
                    "--offline",
                    "--python",
                    "3.12",
                    "--project",
                    "tools/code-ontology",
                    "python",
                    "tools/code-ontology/index.py",
                    *args,
                ],
                destination,
                index_env,
            )

        (evidence / "ontology-update.txt").write_bytes(index("update"))
        changed = set(git("diff", "--name-only").decode().splitlines())
        if (
            not changed
            or not changed.issubset(ONTOLOGY)
            or git("ls-files", "--others", "--exclude-standard")
        ):
            raise ValueError(f"unexpected ontology update paths: {sorted(changed)}")
        git("add", "--update")
        git(
            "commit",
            "-q",
            "-m",
            f"Qualify regenerated Marmish framing and facade from {PIN}",
        )
        (evidence / "ontology-check.txt").write_bytes(index("check", "--ref", "HEAD"))
    if git("status", "--porcelain"):
        raise ValueError("immutable candidate check left a changed source tree")
    result = {
        "assemblySha256": assembly_hash,
        "sourceFiles": len(rows),
        "retainedFiles": len(rows) - len(generated),
        "generated": generated,
        "stagedAssemblyTree": before_tree,
        "regeneratedOntology": sorted(changed),
        "candidateCommit": git("rev-parse", "HEAD").decode().strip(),
        "candidateTree": git("rev-parse", "HEAD^{tree}").decode().strip(),
    }
    (evidence / "result.json").write_text(
        json.dumps(result, sort_keys=True, indent=2) + "\n"
    )
    return result


def rebuild(upstream, out):
    upstream, out = upstream.resolve(), out.resolve()
    out.mkdir(exist_ok=True, parents=True)
    if (
        any(out.iterdir())
        or output(["git", "rev-parse", "HEAD"], upstream).decode().strip() != PIN
    ):
        raise ValueError("expected an empty output and the pinned original checkout")
    env = dict(
        os.environ,
        CARGO_NET_OFFLINE="true",
        RUSTUP_AUTO_INSTALL="0",
        PYTHONDONTWRITEBYTECODE="1",
    )
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "--bin", "oddlang"],
        cwd=ROOT,
        env=env,
        check=True,
    )
    meta = json.loads(
        output(
            [
                "cargo",
                "metadata",
                "--locked",
                "--offline",
                "--no-deps",
                "--format-version",
                "1",
            ],
            ROOT,
            env,
        )
    )
    binary = Path(meta["target_directory"]) / "debug/oddlang"
    original = identities(output(["git", "ls-tree", "-r", "-z", PIN], upstream))
    first = run_build(upstream, out / "candidate", out / "first", binary, original, env)
    with tempfile.TemporaryDirectory(
        prefix="oddlang-independent-rebuild-"
    ) as alternate:
        second = run_build(
            upstream,
            Path(alternate) / "rebuilt-elsewhere",
            out / "second",
            binary,
            original,
            env,
        )
    if first != second:
        raise ValueError("independent candidate results differ")
    for name in ("framing", "facade", "framing-projection", "facade-projection"):
        left, right = out / "first" / name, out / "second" / name
        if {p.name: p.read_bytes() for p in left.iterdir()} != {
            p.name: p.read_bytes() for p in right.iterdir()
        }:
            raise ValueError(f"nonrepeatable recovery or projection: {name}")
    (out / "rebuild.json").write_text(
        json.dumps(
            {"originalCommit": PIN, **first, "independentBuilds": 2},
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        rebuild(args.upstream, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
