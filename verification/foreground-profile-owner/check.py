#!/usr/bin/env python3
"""Exercise generated foreground attachment to an unrelated file-backed station."""

import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ODDLANG = Path(sys.argv[1]).resolve()
DEPS = {
    "clap": {"version": "4.6.7"}, "libc": {"version": "0.2.189"},
    "secp256k1": {"version": "0.30.0"}, "zeroize": {"version": "1.9.1"},
    "rustls": {"version": "0.23.46", "defaultFeatures": False, "features": ["ring", "std", "tls12"]},
    "tokio-rustls": {"version": "0.26.6", "defaultFeatures": False, "features": ["ring", "tls12"]},
    "webpki-roots": {"version": "1.0.11"},
    "serde": {"version": "1.0.229", "features": ["derive"]}, "serde_json": {"version": "1.0.151"},
    "tokio": {"version": "1.54.1", "features": ["io-util", "macros", "net", "rt", "signal", "sync", "time"]},
}
PUBLIC_GENERATOR = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"


def checked(*argv, **opts):
    child = subprocess.run(argv, capture_output=True, timeout=120, **opts)
    if child.returncode:
        raise ValueError((list(map(str, argv)), child.stderr.decode()))


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.recv(size - len(data))
        assert chunk
        data.extend(chunk)
    return bytes(data)


def inspect(path):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(4)
        stream.connect(str(path))
        body = b'{"action":"inspect"}'
        stream.sendall(struct.pack(">I", len(body)) + body)
        stream.shutdown(socket.SHUT_WR)
        size, = struct.unpack(">I", read_exact(stream, 4))
        assert 0 < size <= 128
        assert read_exact(stream, size) == b'{"valid":true,"reading":{"phase":"calibrating"},"problem":null}'
        assert stream.recv(1) == b""


with tempfile.TemporaryDirectory(prefix="fs-", dir="/tmp") as tmp:
    root = Path(tmp).resolve()
    for name in ("state", "runtime", "home"):
        (root / name).mkdir(mode=0o700)
    shutil.copyfile(HERE / "Cargo.lock", root / "Cargo.lock")
    shutil.copyfile(HERE.parent / "local-status-owner/graph.json", root / "owner.json")

    def put(name, data):
        (root / name).write_text(json.dumps(data))

    def pin(name):
        return {"path": name, "sha256": hashlib.sha256((root / name).read_bytes()).hexdigest()}

    args = ["attached", "data", "station", "storage", "route", "relay"]
    put("cli.json", {
        "profile": "command-surface-v1", "package_name": "field-station", "version": "1.0.0",
        "root": {"name": "field-station", "arguments": [
            {"id": key, "long": key, "value_name": key.upper(), "global": True, **kind}
            for key, kind in zip(args, ({}, {"kind": "path"}, {"kind": "string", "default_value": "pond"},
                                       {"kind": "string", "default_value": "disk"}, {"kind": "string"},
                                       {"kind": "string", "repeated": True}))
        ], "groups": [{"id": "parameters", "arguments": args, "multiple": True}],
            "subcommands": [{"name": "serve"}]},
    })
    put("cli-rust.json", {"executable": "field-station", "invalid_exit": 2, "uncovered_exit": 125,
                          "operations": {"": "urn:station:run", "serve": "urn:station:run"},
                          "handlers": {"urn:station:run": ["startup", "attach"]}})
    put("owner-rust.json", {"format": "oddlang-local-status-owner-rust-v1",
                            "executable": "field-station", "socket_option": "channel"})
    field_names = ["header", "token", "label", "change", "phase", "store", "signing", "backup", "device", "pub"]
    put("profile.json", {
        "format": "oddlang-foreground-profile-owner-v1",
        "arguments": dict(foreground="attached", state="data", identity="station", backend="storage",
                          backend_choices=["disk"], route="route", relay="relay"),
        "profile": dict(document="station.json", lock="station.lck", format=3, maximum_bytes=2048,
                        name_limit=32, short_reference_bytes=8, credential_reference_bytes=12,
                        ready="recorded", backend="disk",
                        fields=dict(zip(["format", "profile_id", "name", "transaction", "phase", "backend",
                                         "signing_ref", "data_ref", "device", "public_key"], field_names))),
        "runtime": dict(prefix="fs", binding="locator.json", socket="port", format=4,
                        fields=dict(format="version", profile_id="token", root="profile"),
                        maximum_path_bytes_exclusive=104, reserved_path_bytes=13),
        "provider": dict(kind="private_file_secp256k1", directory="credentials",
                         signing_prefix="attest", data_prefix="backup"),
        "activation": dict(kind="loopback_web_pki_tls", route_value="direct", deadline_ms=1500),
        "diagnostics": {name: name.replace("_", " ") for name in [
            "scope", "io", "invalid_name", "unsafe_directory", "unsafe_file", "already_running",
            "missing_profile", "incompatible_format", "invalid_profile", "not_ready", "backend_mismatch",
            "missing_credential", "invalid_credential", "identity_mismatch", "runtime_collision",
            "runtime_length", "cleanup", "relay_scope", "plaintext_relay", "relay_failed",
            "activation_deadline", "provider_unavailable",
        ]},
    })
    put("profile-rust.json", {"format": "oddlang-foreground-profile-owner-rust-v1",
                              "entry": "attach", "status_owner": ["status", "BoundOwner"]})
    def component(kind, module, graph, rust, names):
        return {"kind": kind, "module": module, "graph": pin(graph), "overlay": pin(rust), "reexports": names}

    package = {
        "format": "oddlang-native-package-v1", "name": "field-station", "version": "1.0.0",
        "edition": "2021", "dependencies": DEPS, "cargoLock": pin("Cargo.lock"),
        "components": [
            component("cli-surface", "cli", "cli.json", "cli-rust.json", ["run_cli"]),
            component("local-status-owner", "status", "owner.json", "owner-rust.json",
                      ["BoundOwner", "SnapshotPublisher", "OwnerError", "run_owner"]),
            component("foreground-profile-owner", "startup", "profile.json", "profile-rust.json", ["attach"]),
        ],
        "binaries": [{"name": "field-station", "entry": ["run_cli"]}],
    }
    put("package.json", package)
    generated = root / "generated"
    checked(ODDLANG, "build", "native-package", "rust", root / "package.json", generated)
    checked("cargo", "build", "--locked", "--offline", "--release", cwd=generated)
    binary = generated / "target/release/field-station"
    profile = root / "state/pond"
    profile.mkdir(mode=0o700)
    credentials = profile / "credentials"
    credentials.mkdir(mode=0o700)
    public = [3, "11"*8, "pond", "22"*8, "recorded", "disk", "33"*12, "44"*12, "55"*8, PUBLIC_GENERATOR]
    manifest = profile / "station.json"
    manifest.write_text(json.dumps(dict(zip(field_names, public))))
    manifest.chmod(0o600)
    signer = credentials / ("attest-" + "33"*12)
    data = credentials / ("backup-" + "44"*12)
    for path, value in ((signer, 1), (data, 2)):
        path.write_bytes(value.to_bytes(32, "big"))
        path.chmod(0o600)
    env = {"PATH": "/usr/bin:/bin", "LANG": "C", "HOME": str(root / "home"), "TMPDIR": str(root / "runtime")}
    relay = socket.socket()
    relay.bind(("127.0.0.1", 0))
    relay.listen()
    relay.settimeout(5)
    relay_port = relay.getsockname()[1]
    cmd = [binary, "--attached", "--data", root / "state", "--route", "direct",
           "--relay", f"wss://127.0.0.1:{relay_port}", "serve"]
    runtime = root / "runtime" / f"fs-{os.geteuid()}-{'11'*8}"

    def rejected(message):
        child = subprocess.run(cmd, cwd=root, env=env, capture_output=True, timeout=5)
        assert (child.returncode, child.stdout, child.stderr) == (1, b"", message.encode() + b"\n"), child.stderr
        assert not runtime.exists()

    signer.write_bytes((2).to_bytes(32, "big"))
    rejected("identity mismatch")
    signer.write_bytes((1).to_bytes(32, "big"))
    data.chmod(0o644)
    rejected("unsafe file")
    data.chmod(0o600)
    with (profile / "station.lck").open("r+b") as lease:
        fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        rejected("already running")

    for _ in range(2):
        child = subprocess.Popen(cmd, env=env, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            endpoint = runtime / "port"
            deadline = time.monotonic() + 6
            while not endpoint.is_socket():
                assert child.poll() is None and time.monotonic() < deadline
                time.sleep(0.01)
            record = json.loads((runtime / "locator.json").read_bytes())
            assert record == {"version": 4, "token": "11"*8, "profile": str(profile)}
            with relay.accept()[0] as handshake:
                handshake.settimeout(5)
                assert read_exact(handshake, 3) == b"\x16\x03\x01"
                inspect(endpoint)
                child.send_signal(signal.SIGTERM)
                assert child.communicate(timeout=5) == (b"", b"")
            assert child.returncode == 0 and not runtime.exists()
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate()

    child = subprocess.Popen(cmd, env=env, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        with relay.accept()[0] as held:
            held.settimeout(5)
            assert read_exact(held, 3) == b"\x16\x03\x01"
            assert child.communicate(timeout=5) == (b"", b"activation deadline\n")
        assert child.returncode == 1 and not runtime.exists()
    finally:
        relay.close()
        if child.poll() is None:
            child.kill()
            child.communicate()

    wrong = json.loads((root / "profile-rust.json").read_bytes())
    wrong["status_owner"] = ["other", "BoundOwner"]
    put("bad-owner.json", wrong)
    package["components"][2]["overlay"] = pin("bad-owner.json")
    put("bad-package.json", package)
    rejected_link = subprocess.run(
        [ODDLANG, "build", "native-package", "rust", root / "bad-package.json", root / "rejected"],
        capture_output=True, timeout=10,
    )
    assert rejected_link.returncode and b"requires a linked local status owner" in rejected_link.stderr
    assert not (root / "rejected").exists()

print("field station attachment, credentials, lease, discovery, shutdown, and reattach passed")
