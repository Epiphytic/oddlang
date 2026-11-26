#!/usr/bin/env python3
"""Verify a generated meter workflow against an independent Unix owner."""

from hashlib import sha256
import json
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile

ODDLANG = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parents[1] / "local-command-client"
SUFFIX = "urn:meter:workflow:"


def checked(*argv, cwd=None):
    result = subprocess.run(argv, cwd=cwd, capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError(result.stderr.decode())
    return result.stdout


def field(name, source):
    return {"id": SUFFIX + "field:" + name, "key": name, "source": source}


def literal(value):
    return {"kind": "literal", "value": value}


def when(path, value):
    return {"path": path.split("."), "equals": value}


def go(label, conditions, step, *, capture=None, delay=0):
    return {"id": SUFFIX + "transition:" + label, "when": conditions, "capture": capture or [],
            "effect": {"kind": "continue", "step": SUFFIX + "step:" + step, "delay_ms": delay}}


def finish(label, conditions, status, diagnostic=""):
    return {"id": SUFFIX + "transition:" + label, "when": conditions,
            "effect": {"kind": "exit", "status": status, "stderr": diagnostic}}


def frame(body):
    return struct.pack("!I", len(body)) + body


def serve(binary, path, steps, expected, *, unit=True):
    with socket.socket(socket.AF_UNIX) as peer:
        peer.bind(str(path))
        peer.listen()
        peer.settimeout(3)
        command = [binary, "read", "--port", path]
        if unit:
            command.extend(["--unit", "kelvin"])
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as client:
            try:
                for request, response in steps:
                    with peer.accept()[0] as connection, connection.makefile("rb") as reader:
                        connection.settimeout(3)
                        actual = reader.read(len(frame(request)) + 1)
                        assert actual == frame(request), actual
                        if response is None:
                            stdout, stderr = client.communicate(timeout=3)
                            break
                        connection.sendall(frame(response))
                        connection.shutdown(socket.SHUT_WR)
                stdout, stderr = client.communicate(timeout=3)
            finally:
                if client.poll() is None:
                    client.kill()
                    client.communicate(timeout=3)
        assert (client.returncode, stdout, stderr) == expected
    path.unlink()


def main():
    with tempfile.TemporaryDirectory(prefix="odw-", dir="/tmp") as directory:
        root = Path(directory)
        def write(name, body):
            path = root / name
            path.write_text(json.dumps(body))
            return {"path": name, "sha256": sha256(path.read_bytes()).hexdigest()}

        cli = json.loads((FIXTURE / "cli.json").read_bytes())
        cli_overlay = {
            "executable": "field-meter", "invalid_exit": 2, "uncovered_exit": 125,
            "operations": {"read": "urn:field:read"},
            "handlers": {"urn:field:read": ["meter", "measure"]},
        }
        workflow = {
            "format": "oddlang-local-command-workflow-v1", "id": "urn:meter:workflow",
            "socket_argument": "port",
            "transport": {"deadline_ms": 1000, "maximum_frame_bytes": 256, "maximum_exchanges": 4},
            "errors": {name: {"exit": status, "stderr": message} for name, status, message in [
                ("binding", 6, "missing argument"), ("io", 7, "socket I/O"), ("peer", 8, "wrong user"),
                ("frame_size", 9, "bad frame size"), ("frame_json", 10, "bad frame JSON"),
                ("deadline", 11, "deadline"), ("protocol", 12, "unexpected reply"),
                ("budget", 13, "exchange budget"),
            ]},
            "entry": SUFFIX + "step:probe",
            "steps": [
                {"id": SUFFIX + "step:probe", "request": [field("probe", literal("probe"))],
                 "transitions": [
                     go("poll", [when("state", "warming")], "probe", delay=1),
                     go("reserve", [when("state", "ready")], "reserve"),
                     finish("later-overlap", [when("state", "ready"), when("mode", "priority")],
                            16, "later transition won"),
                     finish("unexpected", [], 15, "unknown state"),
                 ]},
                {"id": SUFFIX + "step:reserve", "request": [field("reserve", literal("reserve")),
                 field("unit", {"kind": "argument", "id": "unit", "value_kind": "string"})],
                 "transitions": [
                     go("sample", [when("result", "registered")], "sample",
                        capture=[{"name": "slot", "path": ["slot"], "value_kind": {"kind": "lower_hex", "bytes": 2}}]),
                 ]},
                {"id": SUFFIX + "step:sample", "request": [field("sample", literal("measure")),
                 field("slot", {"kind": "captured", "name": "slot"})],
                 "transitions": [
                     go("resample", [when("result", "refresh")], "sample") | {"carry": ["slot"]},
                     finish("busy", [when("result", "unavailable")], 14, "device busy"),
                     finish("complete", [when("result", "done")], 0),
                 ]},
            ],
        }
        lock = (FIXTURE / "Cargo.lock").read_bytes()
        (root / "Cargo.lock").write_bytes(lock)
        manifest = {
            "format": "oddlang-native-package-v1", "name": "field-meter", "version": "1.0.0",
            "edition": "2021", "dependencies": {
                "clap": {"version": "4.6.7"}, "libc": {"version": "0.2.189"},
                "serde": {"version": "1.0.229", "features": ["derive"]},
                "serde_json": {"version": "1.0.151"},
                "tokio": {"version": "1.54.1", "features": ["io-util", "net", "rt", "time"]},
            },
            "cargoLock": {"path": "Cargo.lock", "sha256": sha256(lock).hexdigest()},
            "components": [
                {"kind": "cli-surface", "module": "cli", "graph": write("cli.json", cli),
                 "overlay": write("cli-overlay.json", cli_overlay), "reexports": ["run_cli"]},
                {"kind": "local-command-client", "module": "meter",
                 "graph": write("workflow.json", workflow),
                 "overlay": write("workflow-overlay.json", {
                     "format": "oddlang-local-command-client-rust-v1", "entry": "measure"
                 }), "reexports": ["measure"], "rootReexports": []},
            ],
            "binaries": [{"name": "field-meter", "entry": ["run_cli"]}],
        }
        write("manifest.json", manifest)
        product = root / "native"
        checked(ODDLANG, "build", "native-package", "rust", root / "manifest.json", product)
        checked("cargo", "build", "--locked", "--offline", "--release", cwd=product)
        binary = product / "target/release/field-meter"
        path = root / "sock"
        probe = b'{"probe":"probe"}'
        reserve = b'{"reserve":"reserve","unit":"kelvin"}'
        sample = b'{"sample":"measure","slot":"b00c"}'
        ready = b'{"state":"ready","mode":"priority"}'
        registered = b'{"result":"registered","slot":"b00c"}'
        serve(binary, path, [(probe, b'{"state":"warming"}'), (probe, ready),
                            (reserve, registered), (sample, b'{"result":"done"}')], (0, b"", b""))
        serve(binary, path, [(probe, ready), (reserve, registered),
                            (sample, b'{"result":"unavailable"}')], (14, b"", b"device busy\n"))
        serve(binary, path, [(probe, ready), (reserve, b'{"result":"registered","slot":"oops"}')],
              (12, b"", b"unexpected reply\n"))
        serve(binary, path, [(probe, b'{"state":"warming"}')] * 4, (13, b"", b"exchange budget\n"))
        serve(binary, path, [(probe, b'{"state":"unexpected"}')], (15, b"", b"unknown state\n"))
        serve(binary, path, [(probe, b'{"state":"warming","state":"ready"}')], (10, b"", b"bad frame JSON\n"))
        serve(binary, path, [(probe, None)], (11, b"", b"deadline\n"))
        serve(binary, path, [(probe, b"x" * 257)], (9, b"", b"bad frame size\n"))
        serve(binary, path, [(probe, ready)], (6, b"", b"missing argument\n"), unit=False)
        serve(binary, path, [(probe, ready), (reserve, registered), (sample, b'{"result":"refresh"}'),
                             (sample, b'{"result":"done"}')], (0, b"", b""))

        projection = root / "projection"
        checked(ODDLANG, "build", "local-command-client", "rust", root / "workflow.json",
                root / "workflow-overlay.json", projection)
        catalog = (projection / "workflow-obligations.json").read_bytes()
        lock = json.loads((projection / "projection.lock").read_bytes())
        assert lock["workflowObligationsSha256"] == sha256(catalog).hexdigest()
        obligations = json.loads(catalog)
        assert obligations["captureLifetime"] == "nextStepUnlessExplicitlyCarried"
        assert obligations["obligations"][1]["request"][1]["representation"] == {
            "type": "string", "absent": "reject"
        }
        for change, diagnostic in (
            (lambda doc: doc["steps"][1]["transitions"].append(
                go("stale", [when("result", "already")], "sample")),
             b"cannot carry workflow capture slot before assignment"),
            (lambda doc: doc["steps"][2]["transitions"][0].update(carry=["ghost"]),
             b"undeclared workflow capture ghost"),
            (lambda doc: doc["steps"][0]["transitions"][0]["when"][0].update(equals={}),
             b"invalid workflow reply predicate"),
            (lambda doc: doc["transport"].update(maximum_frame_bytes=2),
             b"minimum workflow request exceeds frame limit"),
        ):
            invalid = json.loads((root / "workflow.json").read_bytes())
            change(invalid)
            write("invalid-workflow.json", invalid)
            bad = subprocess.run([ODDLANG, "build", "local-command-client", "rust", root / "invalid-workflow.json",
                                  root / "workflow-overlay.json", root / "invalid"], capture_output=True, timeout=120)
            assert bad.returncode != 0 and diagnostic in bad.stderr and not (root / "invalid").exists(), (diagnostic, bad.returncode, bad.stderr)
        print("field-meter workflow exact requests, scoped capture, refusal, strict JSON, deadline and budget passed")


if __name__ == "__main__":
    main()
