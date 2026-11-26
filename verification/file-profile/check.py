"""Compile a graph-only credential tool and test its effects with unrelated names."""

import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()


def diagnostic(text):
    return {"exit": 3, "stderr": text.replace("_", " ")}


def run(*args, cwd=None):
    return subprocess.run(args, cwd=cwd, capture_output=True, timeout=120, check=True)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


with tempfile.TemporaryDirectory(prefix="file-profile-") as tmp:
    root = Path(tmp).resolve()

    def pin(name, data):
        path = root / name
        path.write_text(json.dumps(data))
        return {"path": name, "sha256": digest(path)}

    graph = {
        "format": "oddlang-file-profile-v1", "id": "urn:survey:bootstrap",
        "arguments": {"state": "station", "profile": "name", "backend": "medium",
                      "default_profile": "daily", "file_backend": "local"},
        "layout": {"owner": "lease", "manifest": "record.json", "keys": "credentials",
                   "private_subdirectories": ["cache"], "signing_prefix": "signature-",
                   "storage_prefix": "vault-", "transaction_prefix": ".stage-", "name_maximum_bytes": 12,
                   "name_punctuation": "-", "manifest_maximum_bytes": 4096,
                   "directory_mode": 448, "file_mode": 384},
        "manifest": {"version": 9, "pending": "proposed", "ready": "active", "file_backend": "disk",
                     "other_backends": ["external"],
                     "fields": {"version": "v", "profile": "pid", "name": "label", "transaction": "tx",
                                "phase": "stage", "backend": "provider", "signer": "sign",
                                "storage": "locker", "slot": "device", "public": "attestation"}},
        "errors": {error: diagnostic(error) for error in (
            "binding", "state_required", "backend_required", "unsafe_directory", "unsafe_file", "invalid_profile",
            "in_use", "invalid_manifest", "wrong_version", "wrong_backend", "random", "missing_secret",
            "invalid_secret", "invalid_signing_key", "identity_mismatch", "key_readback", "io",
        )},
    }
    overlay = {"format": "oddlang-file-profile-rust-v1", "entry": "prepare_credentials",
               "random": "getrandom", "signing": "secp256k1_x_only"}
    program = {"profile": "command-surface-v1", "package_name": "field-survey", "version": "1.0.0",
               "root": {"name": "field-survey", "subcommands": [{"name": "prepare"}], "arguments": [
                   {"id": id, "long": id, "kind": kind, "global": True}
                   for id, kind in (("station", "path"), ("name", "string"), ("medium", "string"))
               ]}}
    command = {"executable": "field-survey", "invalid_exit": 2, "uncovered_exit": 125,
               "operations": {"prepare": "urn:survey:bootstrap"},
               "handlers": {"urn:survey:bootstrap": ["keys", "prepare_credentials"]}}
    deps = {"clap": ("4.6.7", []), "getrandom": ("0.4.3", []), "libc": ("0.2.189", []),
            "rustix": ("1.1.5", ["fs"]), "secp256k1": ("0.30.0", []), "serde": ("1.0.229", ["derive"]),
            "serde_json": ("1.0.151", []), "zeroize": ("1.9.1", [])}
    fixture = root / "lock-source"
    (fixture / "src").mkdir(parents=True)
    (fixture / "src/lib.rs").touch()
    dependency_lines = [f'{name} = {{version="={version}",features={json.dumps(features)}}}'
                        for name, (version, features) in deps.items()]
    (fixture / "Cargo.toml").write_text('[package]\nname="field-survey"\nversion="1.0.0"\n'
        'edition="2021"\n[dependencies]\n' + "\n".join(dependency_lines) + "\n[workspace]\n")
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=fixture)
    lock = fixture / "Cargo.lock"
    manifest = {"format": "oddlang-native-package-v1", "name": "field-survey", "version": "1.0.0", "edition": "2021",
        "dependencies": {key: {"version": v, "features": f} for key, (v, f) in deps.items()},
        "cargoLock": {"path": str(lock.relative_to(root)), "sha256": digest(lock)},
        "components": [
            {"kind": "cli-surface", "module": "cli", "graph": pin("cli.json", program),
             "overlay": pin("cli-rust.json", command), "reexports": ["run_cli"]},
            {"kind": "file-profile", "module": "keys", "graph": pin("profile.json", graph),
             "overlay": pin("profile-rust.json", overlay), "reexports": ["prepare_credentials"], "rootReexports": []},
        ], "binaries": [{"name": "field-survey", "entry": ["run_cli"]}]}
    pin("package.json", manifest)
    run(CLI, "build", "file-profile", "rust", root / "profile.json", root / "profile-rust.json", root / "projection")
    catalog = root / "projection/profile-obligations.json"
    lock = json.loads((root / "projection/projection.lock").read_bytes())
    assert lock["obligationsSha256"] == digest(catalog)
    package = root / "package"
    run(CLI, "build", "native-package", "rust", root / "package.json", package)
    run("cargo", "+1.97.1", "build", "--locked", "--offline", "--release", cwd=package)
    binary = package / "target/release/field-survey"
    state = root / "state"
    arguments = (binary, "--station", state, "--medium", "local", "prepare")
    first = run(*arguments).stdout
    profile = state / "daily"
    document = json.loads((profile / "record.json").read_bytes())
    assert (document["v"], document["stage"], document["provider"]) == (9, "proposed", "disk")
    assert json.loads(first)["identity"] == document["attestation"]
    files = tuple(profile.joinpath("credentials").iterdir())
    assert len(files) == 2
    assert all(len(file.read_bytes()) == 32 and stat.S_IMODE(file.stat().st_mode) == 0o600 for file in files)
    before = {str(file): (file.stat().st_ino, digest(file)) for file in files}
    assert run(*arguments).stdout == first
    assert {str(file): (file.stat().st_ino, digest(file)) for file in files} == before
    occupied = files[0]
    occupied.chmod(0o644)
    bad = subprocess.run(arguments, capture_output=True, timeout=5)
    assert (bad.returncode, bad.stdout, bad.stderr) == (3, b"", b"unsafe file\n")
    assert digest(occupied) == before[str(occupied)][1]
    assert os.stat(profile).st_mode & 0o777 == 0o700
    graph["layout"]["owner"] = "../lease"
    pin("invalid.json", graph)
    bad = subprocess.run([CLI, "build", "file-profile", "rust", root / "invalid.json",
                          root / "profile-rust.json", root / "invalid"], capture_output=True, timeout=5)
    assert bad.returncode and b"invalid private file profile layout" in bad.stderr
    assert not (root / "invalid").exists()
print("unrelated native profile projection, credential effects and rejection controls passed")
