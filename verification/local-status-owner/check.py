#!/usr/bin/env python3
"""Exercise generated owner admission, status and lifecycle through Unix sockets."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time

FIXTURE = Path(__file__).resolve().parent
ODDLANG = Path(sys.argv[1]).resolve()
GOOD = b'{"valid":true,"reading":{"phase":"calibrating"},"problem":null}'
BAD = b'{"valid":false,"reading":null,"problem":"unrecognized sample"}'
REQUEST = b'{"action":"inspect"}'
DEPS = {
    "clap": {"version": "4.6.7"}, "libc": {"version": "0.2.189"},
    "serde": {"version": "1.0.229", "features": ["derive"]},
    "serde_json": {"version": "1.0.151"},
    "tokio": {"version": "1.54.1", "features": ["io-util", "macros", "net", "rt", "signal", "sync", "time"]},
}


def run(*argv, **kwargs):
    return subprocess.run(argv, check=True, timeout=120, **kwargs)


def process(program, endpoint):
    return subprocess.Popen([program, "--channel", endpoint], stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def wait_socket(path, child):
    deadline = time.monotonic() + 5
    while not path.is_socket():
        if child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError(f"owner failed to bind {path}")
        time.sleep(0.01)


def framed(data):
    return struct.pack(">I", len(data)) + data


def connection(path):
    conn = socket.socket(socket.AF_UNIX)
    conn.settimeout(3)
    conn.connect(str(path))
    return conn


def receive(conn):
    buffer = bytearray()
    while chunk := conn.recv(256):
        buffer.extend(chunk)
        assert len(buffer) <= 132
    return bytes(buffer)


def request(path, wire, expect):
    with connection(path) as peer:
        peer.sendall(wire)
        peer.shutdown(socket.SHUT_WR)
        assert receive(peer) == framed(expect)


with tempfile.TemporaryDirectory(prefix="gauge-", dir="/tmp") as directory:
    root = Path(directory).resolve()
    for file in ("graph.json", "Cargo.lock"):
        shutil.copyfile(FIXTURE / file, root / file)

    def pin(name):
        return {"path": name, "sha256": hashlib.sha256((root / name).read_bytes()).hexdigest()}

    binding = {"format": "oddlang-local-status-owner-rust-v1", "executable": "river-gauge",
               "socket_option": "channel"}
    (root / "rust.json").write_text(json.dumps(binding))
    manifest = {
        "format": "oddlang-native-package-v1", "name": "river-gauge", "version": "1.0.0",
        "edition": "2021", "dependencies": DEPS, "cargoLock": pin("Cargo.lock"),
        "components": [{"kind": "local-status-owner", "module": "owner", "graph": pin("graph.json"),
                        "overlay": pin("rust.json"), "reexports": ["run_owner", "BoundOwner", "SnapshotPublisher", "OwnerError"]}],
        "binaries": [{"name": "river-gauge", "entry": ["run_owner"]}],
    }
    (root / "package.json").write_text(json.dumps(manifest))
    build = root / "generated"
    run(ODDLANG, "build", "native-package", "rust", root / "package.json", build)
    run("cargo", "build", "--release", "--locked", "--offline", cwd=build)
    program = build / "target/release/river-gauge"
    endpoint = root / "pipe"
    child = process(program, endpoint)
    try:
        wait_socket(endpoint, child)
        assert stat.S_IMODE(endpoint.stat().st_mode) == 0o600
        request(endpoint, framed(REQUEST), GOOD)
        for wire in (
            framed(b'["inspect"]'), framed(b'{"action":"ignore"}'),
            framed(b'{"action":"inspect","duplicate":true}'), framed(b'{"action":"inspect","action":"inspect"}'),
            struct.pack(">I", 129), struct.pack(">I", 0), struct.pack(">I", 5) + b"{}",
        ):
            request(endpoint, wire, BAD)
        with connection(endpoint) as idle1, connection(endpoint) as idle2:
            idle1.sendall(b"\x00")
            idle2.sendall(b"\x00")
            with connection(endpoint) as overloaded:
                assert receive(overloaded) == b""
            assert receive(idle1) == framed(BAD)
            assert receive(idle2) == framed(BAD)
        other = subprocess.run([program, "--channel", endpoint], capture_output=True, timeout=4)
        assert (other.returncode, other.stderr) == (1, b"gauge endpoint occupied\n")
        request(endpoint, framed(REQUEST), GOOD)
        child.send_signal(signal.SIGTERM)
        assert child.communicate(timeout=5) == (b"", b"")
        assert child.returncode == 0 and not endpoint.exists()
    finally:
        if child.poll() is None:
            child.kill()
            child.communicate()

    unsafe = root / "shared"
    unsafe.mkdir(mode=0o755)
    rejected = subprocess.run([program, "--channel", unsafe / "pipe"], capture_output=True, timeout=4)
    assert (rejected.returncode, rejected.stderr) == (1, b"gauge directory is not private\n")
    endpoint.write_bytes(b"owned by fixture")
    rejected = subprocess.run([program, "--channel", endpoint], capture_output=True, timeout=4)
    assert rejected.returncode and endpoint.read_bytes() == b"owned by fixture"
    endpoint.unlink()

    # Replacing the pathname after binding does not transfer cleanup ownership.
    child = process(program, endpoint)
    try:
        wait_socket(endpoint, child)
        endpoint.unlink()
        endpoint.write_bytes(b"replacement")
        child.send_signal(signal.SIGINT)
        assert child.communicate(timeout=5) == (b"", b"")
        assert child.returncode == 0 and endpoint.read_bytes() == b"replacement"
    finally:
        if child.poll() is None:
            child.kill()
            child.communicate()

    endpoint.unlink()
    child = process(program, endpoint)
    try:
        wait_socket(endpoint, child)
        endpoint.unlink()
        with socket.socket(socket.AF_UNIX) as replacement:
            replacement.bind(str(endpoint))
            identity = endpoint.stat().st_ino
            child.send_signal(signal.SIGTERM)
            assert child.communicate(timeout=5) == (b"", b"")
            assert child.returncode == 0 and endpoint.stat().st_ino == identity
    finally:
        if child.poll() is None:
            child.kill()
            child.communicate()

    tests = build / "tests"
    tests.mkdir()
    shutil.copyfile(FIXTURE / "consumer.rs", tests / "consumer.rs")
    run("cargo", "test", "--release", "--locked", "--offline", "--test", "consumer", cwd=build,
        env=dict(os.environ, GAUGE_TEST_DIR=str(root)))

    binding["routed_requests"] = True
    (root / "rust.json").write_text(json.dumps(binding))
    manifest["components"][0]["overlay"] = pin("rust.json")
    (root / "package.json").write_text(json.dumps(manifest))
    routed = root / "routed-build"
    run(ODDLANG, "build", "native-package", "rust", root / "package.json", routed)
    (routed / "tests").mkdir()
    shutil.copyfile(FIXTURE / "routed_consumer.rs", routed / "tests/routed_consumer.rs")
    run("cargo", "test", "--release", "--locked", "--offline", "--test", "routed_consumer", cwd=routed,
        env=dict(os.environ, GAUGE_TEST_DIR=str(root)))

    graph = json.loads((root / "graph.json").read_bytes())
    graph["lifecycle"]["transitions"][0]["from"] = "not-reachable"
    (root / "mutant.json").write_text(json.dumps(graph))
    mutant = subprocess.run(
        [ODDLANG, "build", "local-status-owner", "rust", root / "mutant.json", root / "rust.json", root / "invalid"],
        capture_output=True, timeout=5,
    )
    assert mutant.returncode and b"unreachable local status transition" in mutant.stderr
    assert not (root / "invalid").exists()

    graph = json.loads((root / "graph.json").read_bytes())
    graph["lifecycle"]["stopping"] = "z" * 128
    (root / "mutant.json").write_text(json.dumps(graph))
    mutant = subprocess.run(
        [ODDLANG, "build", "local-status-owner", "rust", root / "mutant.json", root / "rust.json", root / "invalid"],
        capture_output=True, timeout=5,
    )
    assert mutant.returncode and b"declared local status frame exceeds listener bound" in mutant.stderr
    assert not (root / "invalid").exists()


print("native gauge owner passed snapshot publication, malformed frame, concurrency, signal and ownership checks")
