#!/usr/bin/env python3
"""Exercise a generated field-meter command against an independent Unix peer."""

import hashlib
import json
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent
ODDLANG = Path(sys.argv[1]).resolve()
DEPS = {
    "clap": {"version": "4.6.7"},
    "libc": {"version": "0.2.189"},
    "serde": {"version": "1.0.229", "features": ["derive"]},
    "serde_json": {"version": "1.0.151"},
    "tokio": {"version": "1.54.1", "features": ["io-util", "net", "rt", "time"]},
}
REQUEST = b'{"verb":"read","sensor":10}'


def checked(*args, cwd=None):
    return subprocess.run(args, cwd=cwd, check=True, capture_output=True, timeout=120)


def capture(process):
    stdout, stderr = process.communicate(timeout=5)
    return process.returncode, stdout, stderr


def receive(connection, count):
    body = b""
    while len(body) < count:
        chunk = connection.recv(count - len(body))
        assert chunk, body
        body += chunk
    return body


def reply(body):
    return struct.pack("!I", len(body)) + body


def exchange(binary, path, wire, expected):
    with socket.socket(socket.AF_UNIX) as listener:
        listener.settimeout(3)
        listener.bind(str(path))
        listener.listen(1)
        argv = [binary, "read", "--port", path]
        with subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(3)
                assert receive(connection, 4) == struct.pack("!I", len(REQUEST))
                assert receive(connection, len(REQUEST)) == REQUEST
                assert connection.recv(1) == b"", "client did not half-close"
                if wire is not None:
                    connection.sendall(wire[:2])
                    connection.sendall(wire[2:])
                    connection.shutdown(socket.SHUT_WR)
                assert capture(process) == expected
    path.unlink()


with tempfile.TemporaryDirectory(prefix="fmc-", dir="/tmp") as directory:
    root = Path(directory)
    for file in ("cli.json", "graph.json", "Cargo.lock"):
        shutil.copyfile(ROOT / file, root / file)

    def pin(name):
        return {"path": name, "sha256": hashlib.sha256((root / name).read_bytes()).hexdigest()}

    def write(name, value):
        (root / name).write_text(json.dumps(value))

    write("binding.json", {"format": "oddlang-local-command-client-rust-v1", "entry": "measure"})
    write("cli-rust.json", {"executable": "field-meter", "invalid_exit": 2, "uncovered_exit": 125,
                           "operations": {"read": "urn:field:read"},
                           "handlers": {"urn:field:read": ["device", "measure"]}})
    write("manifest.json", {
        "format": "oddlang-native-package-v1", "name": "field-meter", "version": "1.0.0",
        "edition": "2021", "dependencies": DEPS, "cargoLock": pin("Cargo.lock"),
        "components": [
            {"kind": "cli-surface", "module": "cli", "graph": pin("cli.json"),
             "overlay": pin("cli-rust.json"), "reexports": ["run_cli"]},
            {"kind": "local-command-client", "module": "device", "graph": pin("graph.json"),
             "overlay": pin("binding.json"), "reexports": ["measure"], "rootReexports": []},
        ],
        "binaries": [{"name": "field-meter", "entry": ["run_cli"]}],
    })
    checked(ODDLANG, "build", "native-package", "rust", root / "manifest.json", root / "generated")
    checked("cargo", "build", "--locked", "--offline", cwd=root / "generated")
    binary = root / "generated/target/debug/field-meter"
    path = root / "s"
    exchange(binary, path, reply(b'{"ready":true,"reading":{"z":1,"a":2},"notice":null}'),
             (0, b'{"reading":{"a":2,"z":1},"ready":true}\n', b""))
    exchange(binary, path, reply(b'{"notice":"invalid cartridge","ready":false}'),
             (3, b'{"reading":null,"ready":false,"notice":"invalid cartridge"}\n', b""))
    exchange(binary, path, struct.pack("!I", 0), (9, b"", b"invalid sensor frame size\n"))
    exchange(binary, path, struct.pack("!I", 129), (9, b"", b"invalid sensor frame size\n"))
    exchange(binary, path, reply(b'{"ready":true,"extra":1}'), (10, b"", b"invalid sensor response\n"))
    exchange(binary, path, reply(b'{"ready":"true"}'), (10, b"", b"invalid sensor response\n"))
    exchange(binary, path, struct.pack("!I", 9) + b"{}", (7, b"", b"sensor I/O\n"))
    exchange(binary, path, None, (11, b"", b"sensor timed out\n"))
    invalid = subprocess.run([binary, "read", "--port", path, "--unit", "metric"], capture_output=True)
    assert (invalid.returncode, invalid.stdout, invalid.stderr) == (4, b"", b"invalid unit\n")
    missing = subprocess.run([binary, "read"], capture_output=True)
    assert (missing.returncode, missing.stdout, missing.stderr) == (6, b"", b"unbound sensor\n")

    graph = json.loads((root / "graph.json").read_bytes())
    graph["transport"]["maximum_frame_bytes"] = 1
    write("mutant.json", graph)
    mutant = subprocess.run([ODDLANG, "build", "local-command-client", "rust", root / "mutant.json",
                             root / "binding.json", root / "invalid"], capture_output=True)
    assert mutant.returncode and b"constant request exceeds frame limit" in mutant.stderr
    assert not (root / "invalid").exists()

print("field-meter IPC: eight framed interactions, two argument errors and one graph rejection passed")
