"""Compare unrelated legacy record and package artifacts across two compilers."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

SOURCE = """#[derive(Clone, Debug, Default)]
pub struct Contour {
    pub label: String,
    pub cells: usize,
}
"""
ID = "urn:terrain:contour"
OLD, NEW = (Path(value).resolve() for value in sys.argv[1:])


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def run(*command, cwd):
    subprocess.run(command, cwd=cwd, check=True)


def artifacts(cli, root):
    root.mkdir()
    source = root / "source.rs"
    source.write_text(SOURCE)
    write(root / "selection.json", {
        "format": "oddlang-rust-root-record-selection-v1",
        "sourceSha256": digest(source.read_bytes()),
        "id": "urn:terrain:root",
        "records": {"Contour": ID},
    })
    run(cli, "recover", "public-root-records", "source.rs", "selection.json", "model", cwd=root)
    run(cli, "build", "public-records", "rust", "model/graph.json", "model/rust.json", "projection", cwd=root)

    def pin(path):
        return {"path": path, "sha256": digest((root / path).read_bytes())}

    (root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "terrain-fixture"\nversion = "0.0.0"\n')
    write(root / "package.json", {
        "format": "oddlang-native-package-v1", "name": "terrain-fixture", "version": "0.0.0",
        "edition": "2024", "cargoLock": pin("Cargo.lock"),
        "components": [{"kind": "public-records", "module": "contours", "reexports": ["Contour"],
                        "graph": pin("model/graph.json"), "overlay": pin("model/rust.json")}],
    })
    run(cli, "build", "native-package", "rust", "package.json", "product", cwd=root)
    return {name: (root / name).read_bytes() for name in (
        "model/graph.json", "model/rust.json", "model/recovery.lock",
        "projection/records.rs", "projection/projection.lock",
        "product/Cargo.toml", "product/Cargo.lock", "product/package.lock",
        "product/src/lib.rs", "product/src/group_0/mod.rs", "product/src/group_0/__projection_0.rs",
    )}


with tempfile.TemporaryDirectory(prefix="odd-record-composition-") as temporary:
    work = Path(temporary)
    before = artifacts(OLD, work / "before")
    after = artifacts(NEW, work / "after")
    if before != after:
        changed = [name for name, data in before.items() if after[name] != data]
        raise ValueError(f"legacy output changed: {changed}")
    print(json.dumps({path: digest(value) for path, value in after.items()}, sort_keys=True))
