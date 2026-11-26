"""Assemble two unrelated graphic formats into one native package and exercise its public API."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

CLI = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parents[2] / "tests/fixtures/json-envelope"


def run(*args, cwd=None, error=None, env=None):
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, env=env)
    if error is None:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode != 0 and error in result.stderr, result.stderr


def pin(path, root):
    return {"path": str(path.relative_to(root)), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


with tempfile.TemporaryDirectory(prefix="native-image-package-") as directory:
    root = Path(directory)
    inputs = root / "inputs"
    inputs.mkdir()
    lock = inputs / "Cargo.lock"
    shutil.copyfile(FIXTURE / "Cargo.lock", lock)
    graph = inputs / "domain.ttl"
    shutil.copyfile(FIXTURE / "graph/domain.ttl", graph)
    binding = json.loads((FIXTURE / "binding.json").read_bytes())
    cargo = tomllib.loads((FIXTURE / "Cargo.toml").read_text())
    deps = {}
    for name, value in cargo["dependencies"].items():
        deps[name] = {"version": value.lstrip("=")} if isinstance(value, str) else {
            **value, "version": value["version"].lstrip("=")}
    manifest = dict(format="oddlang-native-package-v1", name=cargo["package"]["name"],
                    version=cargo["package"]["version"], edition="2024", dependencies=deps,
                    cargoLock=pin(lock, inputs), components=[])
    for group, names in (("lib", ("Thumbnail", "Bounds", "ReadError")),
                         ("lib", ("Sticker", "StickerSize", "StickerError"))):
        overlay = inputs / (names[0] + ".json")
        overlay.write_text(json.dumps(dict(binding, codecType=names[0], limitsType=names[1], errorType=names[2])))
        manifest["components"].append(dict(kind="json-envelope", module=group, graph=pin(graph, inputs),
                                           overlay=pin(overlay, inputs), reexports=list(names)))
    spec = inputs / "manifest.json"
    def build(data, name, error=None):
        spec.write_text(json.dumps(data))
        output = root / name
        run(CLI, "build", "native-package", "rust", spec, output, error=error)
        if error and name != "first":
            assert not output.exists()
        return output

    first = build(manifest, "first")
    second = build(manifest, "second")
    def files(path):
        return {p.relative_to(path): p.read_bytes() for p in path.rglob("*") if p.is_file()}
    assert files(first) == files(second)
    assert json.loads((first / "package.lock").read_bytes())["generatedFiles"] == {
        p.relative_to(first).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in (first / "src").rglob("*.rs")
    }
    assert set(p.suffix for p in inputs.iterdir()) == {".ttl", ".json", ".lock"}
    build(manifest, "first", "output already exists")
    mutated = copy.deepcopy(manifest)
    mutated["components"][1]["reexports"] = ["Thumbnail"]
    build(mutated, "duplicate", "conflicting reexport")
    mutated["components"][1]["reexports"] = ["Typo"]
    build(mutated, "typo", "unknown public projected type")
    mutated = copy.deepcopy(manifest)
    mutated["cargoLock"]["sha256"] = "a"*64
    build(mutated, "stale", "digest mismatch")
    outside = root / "outside.ttl"
    shutil.copy(graph, outside)
    (inputs / "alias.ttl").symlink_to(outside)
    mutated = copy.deepcopy(manifest)
    mutated["components"][0]["graph"]["path"] = "alias.ttl"
    build(mutated, "escape", "outside package")
    mutated = copy.deepcopy(manifest)
    mutated["version"] = "8.0.0"
    build(mutated, "stale-lock", "Cargo.lock validation")
    if sys.platform in ("linux", "darwin"):
        spec.write_text(json.dumps(manifest))
        tools = root / "tools"
        tools.mkdir()
        competing = root / "competing"
        cargo = tools / "cargo"
        cargo.write_text('#!/bin/sh\nmkdir "$COMPETING" || exit 1\nexec "$REAL_CARGO" "$@"\n')
        cargo.chmod(0o755)
        env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                   COMPETING=str(competing), REAL_CARGO=shutil.which("cargo"))
        run(CLI, "build", "native-package", "rust", spec, competing, error="publish fresh package", env=env)
        assert competing.is_dir() and not list(competing.iterdir())
    (first / "tests").mkdir()
    (first / "tests/consumer.rs").write_text("""
use thumbnail_consumer::{Thumbnail, Sticker, StickerError, Bounds};
#[test]
fn shared_wire() {
    assert_eq!(Bounds::PIXELS, 4);
    let wire = Thumbnail::write(&[0,255]).unwrap();
    assert_eq!(Sticker::read(&wire).unwrap(), [0,255]);
    assert_eq!(Sticker::read(b"{}"), Err(StickerError::Malformed));
}
""")
    run("cargo", "+1.97.1", "test", "--locked", "--offline", "--quiet", cwd=first)
print("Two projections in one private module, clean replay, negative controls and downstream Rust passed.")
