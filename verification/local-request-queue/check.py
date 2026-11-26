#!/usr/bin/env python3
"""Build a pinned queue package and exercise it in two separate processes."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


HERE = Path(__file__).resolve().parent
CLI = Path(sys.argv[1]).resolve()
CARGO = ("cargo", "+1.97.1")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


with tempfile.TemporaryDirectory(prefix="oddlang-river-gauge-queue-") as temporary:
    root = Path(temporary)

    def pin(name, value=None):
        path = root / name
        if value is not None:
            path.write_text(json.dumps(value))
        return {"path": name, "sha256": digest(path)}

    shutil.copyfile(HERE / "Cargo.lock", root / "Cargo.lock")
    graph = dict(format="oddlang-local-request-queue-v1", id="urn:gauge:reading-queue",
                 accountBytes=4, roomBytes=8, idBytes=16, maximumFrameBytes=4)
    overlay = dict(format="oddlang-local-request-queue-rust-v1", storage="sqlite")
    manifest = dict(format="oddlang-native-package-v1", name="gauge-reading-queue", version="0.0.0",
                    edition="2021", cargoLock=pin("Cargo.lock"), dependencies={
                        "rusqlite": {"version": "0.40.1", "defaultFeatures": False,
                                     "features": ["bundled-sqlcipher-vendored-openssl"]},
                        "sha2": {"version": "0.10.9"},
                    }, registryPatches={
                        "libsqlite3-sys": {"git": "https://github.com/rusqlite/rusqlite.git",
                                           "revision": "5ae7fdf83085c595fd54f977b3a56ccacabaf16b"},
                    }, components=[dict(kind="local-request-queue", module="queue", visibility="public",
                                       graph=pin("queue.json", graph), overlay=pin("rust.json", overlay),
                                       reexports=["Account", "Room", "Id", "QueueScope", "RequestKind", "Request",
                                                  "QueueReceipt", "Enqueued", "QueuedFrame", "QueueError", "LocalQueue"])])
    path = root / "package.json"
    path.write_text(json.dumps(manifest))
    output = root / "out"
    subprocess.run((CLI, "build", "native-package", "rust", path, output), check=True)
    (output / "examples").mkdir()
    shutil.copyfile(HERE / "consumer.rs", output / "examples/observe.rs")
    env = {**os.environ, "CARGO_ENCODED_RUSTFLAGS": "-Dwarnings"}
    subprocess.run((*CARGO, "build", "--example", "observe", "--offline", "--locked", "--quiet"),
                   cwd=output, env=env, check=True)
    target = output / env.get("CARGO_TARGET_DIR", "target")
    observer = target / "debug/examples/observe"
    database = root / "readings.sqlite3"
    for phase in ("write", "recover"):
        subprocess.run((observer, phase, database), check=True)
    assert database.read_bytes()[:16] != b"SQLite format 3\x00"
    for bound in (0, 1_048_577):
        graph["maximumFrameBytes"] = bound
        manifest["components"][0]["graph"] = pin("queue.json", graph)
        path.write_text(json.dumps(manifest))
        result = subprocess.run((CLI, "build", "native-package", "rust", path, root / "invalid"),
                                capture_output=True, text=True)
        assert result.returncode and "invalid local request queue graph" in result.stderr, result.stderr
    print("durable local queue fixture passed")
