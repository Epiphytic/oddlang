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
        "safety": {"preflightFileType": "regular-or-final-symlink", "nonblockingOpen": True,
                   "hashCeiling": "exact-size-plus-one"},
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
        {"profile": dict(profile, safety=dict(profile["safety"], nonblockingOpen=False))},
        {"profile": dict(profile, open=["read-only", "close-on-exec"])},
        {"profile": dict(profile, guards=list(reversed(profile["guards"])))},
        {"profile": dict(profile, digest={"algorithm": "sha256", "chunkBytes": 0})},
    ):
        write(root / "graph.json", dict(graph, **edit))
        result = subprocess.run([CLI, "build", "read-only-file", "rust", root / "graph.json",
                                 root / "rust.json", root / "negative"], capture_output=True)
        assert result.returncode and not (root / "negative").exists(), result

    def pin(path, value=None):
        file = root / path
        if value is not None:
            write(file, value)
        return {"path": path, "sha256": hashlib.sha256(file.read_bytes()).hexdigest()}

    fault, alias = "urn:fixture:Fault", "urn:fixture:Result"
    invalid = profile["errors"]["descriptor"]["id"]
    io = profile["errors"]["io"]["id"]
    error_graph = {
        "format": "oddlang-finite-error-domain-v1", "id": "urn:fixture:error-domain",
        "error": fault, "result": alias, "categories": {},
        "variants": {
            invalid: {"payload": {"kind": "static-text"},
                      "message": {"kind": "insertion", "prefix": "invalid: ", "suffix": ""},
                      "source": False, "from": False},
            io: {"payload": {"kind": "io-error"}, "message": {"kind": "literal", "text": "I/O"},
                 "source": True, "from": True},
        },
    }
    error_binding = {
        "format": "oddlang-finite-error-rust-v1", "categoryTypes": {}, "variantOrder": [invalid, io],
        "names": {fault: "Fault", alias: "Result", invalid: "Rejected", io: "Io"},
    }
    manifest = {
        "format": "oddlang-native-package-v1", "name": "inert-sample", "version": "0.1.0",
        "edition": "2021", "cargoLock": pin("Cargo.lock"),
        "dependencies": {library: {"version": version} for library, version in
                         [("libc", "0.2.189"), ("sha2", "0.10.9")]},
        "components": [
            {"kind": "finite-error-domain", "module": "fault", "reexports": ["Fault", "Result"],
             "graph": pin("error.json", error_graph), "overlay": pin("error-rust.json", error_binding)},
            {"kind": "read-only-file", "module": "descriptor", "reexports": [],
             "crateReexports": ["open_sample", "read_sample", "check_sample"],
             "graph": pin("graph.json", graph), "overlay": pin("rust.json", binding)},
        ],
    }
    pin("package.json", manifest)
    run(CLI, "build", "native-package", "rust", root / "package.json", root / "native", cwd=root)
    run("cargo", "+1.97.1", "check", "--offline", "--locked", cwd=root / "native")
    for label, modify, error in (
        ("io-identity", lambda data: data.update(ioVariant="Rejected"), "invalid read-only file Rust binding"),
        ("unresolved-variant", lambda data: data.update(ioVariant="Missing"),
         "error role does not match its bound native variant"),
        ("guessed-type", lambda data: data.update(error=["crate", "Missing"]),
         "error role does not match its bound native variant"),
    ):
        invalid_binding = dict(binding)
        modify(invalid_binding)
        manifest["components"][1]["overlay"] = pin("rust.json", invalid_binding)
        pin("package.json", manifest)
        denied = subprocess.run([CLI, "build", "native-package", "rust", root / "package.json", root / label],
                                capture_output=True, text=True)
        assert denied.returncode and error in denied.stderr and not (root / label).exists(), denied
