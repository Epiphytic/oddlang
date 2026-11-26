"""Check a renamed inert-file consumer against its descriptor policy."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent


def write(path, obj):
    path.write_text(json.dumps(obj, sort_keys=True) + "\n")


def run(*cmd, cwd):
    subprocess.run(cmd, cwd=cwd, check=True)


with tempfile.TemporaryDirectory() as scratch:
    root = Path(scratch)
    profile = {
        "id": "urn:fixture:archive:file",
        "modeMask": 511,
        "digest": {"algorithm": "sha256", "chunkBytes": 17},
        "open": ["read-only", "no-follow-final-link", "close-on-exec"],
        "guards": ["regular", "links", "maximum-size", "exact-size",
                   "effective-owner", "masked-mode", "digest"],
        "readCeiling": "requested-size-plus-one",
        "errors": {
            "io": {"id": "urn:fixture:error:io"},
            **{name: {"id": "urn:fixture:error:invalid", "code": code} for name, code in (
                ("descriptor", "unsafe-file"), ("boundedRead", "read-overflow"),
                ("metadata", "wrong-metadata"), ("digest", "wrong-digest"),
            )},
        },
    }
    graph = {"format": "oddlang-read-only-file-v1", "maximumLinks": 1, "profile": profile}
    binding = {
        "format": "oddlang-read-only-file-rust-v1",
        "open": "open_sample", "read": "read_sample", "verify": "check_sample",
        "error": ["crate", "Fault"], "invalidVariant": "Rejected", "ioVariant": "Io",
    }
    write(root / "graph.json", graph)
    write(root / "rust.json", binding)
    out = root / "projection"
    run(CLI, "build", "read-only-file", "rust", root / "graph.json", root / "rust.json", out, cwd=root)
    manifest = '[package]\nname="inert-sample"\nversion="0.1.0"\nedition="2021"\n'
    (root / "Cargo.toml").write_text(manifest + '[dependencies]\nlibc="=0.2.189"\nsha2="=0.10.9"\n')
    (root / "src").mkdir()
    (root / "src/lib.rs").write_bytes((HERE / "caller.rs").read_bytes())
    (root / "src/file.rs").write_bytes((out / "file.rs").read_bytes())
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)
    run("cargo", "+1.97.1", "test", "--locked", "--offline", cwd=root)
    observation = {"format": "oddlang-read-only-file-observation-v1",
                   "oneLink": "accepted", "twoLinks": "accepted", "threeLinks": "descriptor"}
    observation_path = root / "observation.json"
    write(observation_path, observation)
    write(root / "selection.json", {
        "format": "oddlang-read-only-file-selection-v1",
        "observationSha256": hashlib.sha256(observation_path.read_bytes()).hexdigest(),
        "profile": profile,
    })
    run(CLI, "recover", "read-only-file", observation_path, root / "selection.json", root / "recovered", cwd=root)
    assert json.loads((root / "recovered/graph.json").read_bytes()) == dict(graph, maximumLinks=2)
    for edit in (
        {"maximumLinks": 0},
        {"profile": dict(profile, modeMask=4095)},
        {"profile": dict(profile, open=["read-only", "close-on-exec"])},
        {"profile": dict(profile, guards=list(reversed(profile["guards"])))},
        {"profile": dict(profile, digest={"algorithm": "sha256", "chunkBytes": 0})},
    ):
        write(root / "graph.json", dict(graph, **edit))
        result = subprocess.run([CLI, "build", "read-only-file", "rust", root / "graph.json",
                                 root / "rust.json", root / "negative"], capture_output=True)
        assert result.returncode and not (root / "negative").exists(), result
