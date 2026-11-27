#!/usr/bin/env python3
"""Exercise a generated field-meter command against an independent Unix peer."""

import errno
import hashlib
import json
import os
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


def field_meter(workspace):
    checked("cargo", "build", "--release", "--locked", "--offline", cwd=workspace)
    return workspace / os.environ.get("CARGO_TARGET_DIR", "target") / "release/field-meter"


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


def exchange(binary, path, wire, expected, *, args=None, env=None, request=REQUEST):
    with socket.socket(socket.AF_UNIX) as listener:
        listener.settimeout(3)
        listener.bind(str(path))
        listener.listen(1)
        argv = [binary, "read", "--port", path] if args is None else [binary, *args]
        with subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env) as process:
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(3)
                assert receive(connection, 4) == struct.pack("!I", len(request))
                assert receive(connection, len(request)) == request
                assert connection.recv(1) == b"", "client did not half-close"
                if wire is not None:
                    connection.sendall(wire[:2])
                    connection.sendall(wire[2:])
                    try:
                        connection.shutdown(socket.SHUT_WR)
                    except OSError as error:
                        if error.errno != errno.ENOTCONN:
                            raise
                assert capture(process) == expected
    path.unlink()


checked("cargo", "+1.97.1", "run", "--offline", "--locked", "--quiet",
        "--manifest-path", ROOT.parent / "provider-validation/Cargo.toml",
        "--bin", "field-meter-projection", "--", ODDLANG)

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
    manifest = {
        "format": "oddlang-native-package-v1", "name": "field-meter", "version": "1.0.0",
        "edition": "2021", "dependencies": DEPS, "cargoLock": pin("Cargo.lock"),
        "components": [
            {"kind": "cli-surface", "module": "cli", "graph": pin("cli.json"),
             "overlay": pin("cli-rust.json"), "reexports": ["run_cli"]},
            {"kind": "local-command-client", "module": "device", "graph": pin("graph.json"),
             "overlay": pin("binding.json"), "reexports": ["measure"], "rootReexports": []},
        ],
        "binaries": [{"name": "field-meter", "entry": ["run_cli"]}],
    }
    write("manifest.json", manifest)
    checked(ODDLANG, "build", "native-package", "rust", root / "manifest.json", root / "generated")
    binary = field_meter(root / "generated")
    path = root / "s"
    exchange(binary, path, reply(b'{"ready":true,"reading":{"z":1,"a":2},"notice":null}'),
             (0, b'{"reading":{"a":2,"z":1},"ready":true}\n', b""))
    exchange(binary, path, reply(b'{"ready":true}') + reply(b'{}'),
             (0, b'{"reading":null,"ready":true}\n', b""))
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

    missing_socket = subprocess.run([binary, "read", "--port", path], capture_output=True)
    assert (missing_socket.returncode, missing_socket.stdout, missing_socket.stderr) == (7, b"", b"sensor I/O\n")

    dynamic = json.loads((root / "graph.json").read_bytes())
    dynamic["request_id"] = "urn:field:request"
    dynamic["request"] = json.loads((ROOT / "typed-request.json").read_bytes())
    dynamic_cli = json.loads((root / "cli.json").read_bytes())
    dynamic_cli["root"]["arguments"].extend([
        {"id": "sensor", "long": "sensor", "kind": "u64", "global": True},
        {"id": "label", "long": "label", "kind": "string", "global": True},
        {"id": "waiting", "long": "waiting", "global": True},
        {"id": "probe", "long": "probe", "kind": "string", "global": True, "repeated": True},
    ])
    for field in dynamic_cli["root"]["arguments"]:
        if field["id"] == "sensor":
            field["default_value"] = "7"
        if field["id"] == "probe":
            field["default_value"] = "backup"
    write("dynamic.json", dynamic)
    write("dynamic-cli.json", dynamic_cli)
    dynamic_package = json.loads((root / "manifest.json").read_bytes())
    dynamic_package["components"][0]["graph"] = pin("dynamic-cli.json")
    dynamic_package["components"][1]["graph"] = pin("dynamic.json")
    write("dynamic-package.json", dynamic_package)
    built = subprocess.run([ODDLANG, "build", "native-package", "rust", root / "dynamic-package.json",
                            root / "dynamic-client"], capture_output=True, timeout=120)
    assert built.returncode == 0, built.stderr
    dynamic_bin = field_meter(root / "dynamic-client")
    args = ["read", "--port", path, "--sensor", "18446744073709551615", "--label", 'site"3',
            "--waiting", "--probe", "b", "--probe", "a"]
    valid = br'{"verb":"read","sensor":18446744073709551615,"label":"site\"3","waiting":true,"probes":["b","a"],"comment":null}'
    padding = "x" * (128 - len(valid))
    args[6] += padding
    filled = valid.replace(b'3","waiting"', b'3' + padding.encode() + b'","waiting"')
    assert len(filled) == 128
    exchange(dynamic_bin, path, reply(b'{"ready":true}'), (0, b'{"reading":null,"ready":true}\n', b""),
             args=args, request=filled)
    exchange(dynamic_bin, path, reply(b'{"ready":true}'), (0, b'{"reading":null,"ready":true}\n', b""),
             args=["read", "--port", path, "--sensor", "0", "--label", "測/<\t\x01"],
             request=b'{"verb":"read","sensor":0,"label":"\xe6\xb8\xac/<\\t\\u0001","waiting":false,"probes":[],"comment":null}')
    args[6] += "x"
    for invalid_args, expected in (
        (["read", "--port", path, "--label", "a"], (6, b"", b"unbound sensor\n")),
        (args, (9, b"", b"invalid sensor frame size\n")),
    ):
        bad = subprocess.run([dynamic_bin, *invalid_args], capture_output=True, timeout=5)
        assert (bad.returncode, bad.stdout, bad.stderr) == expected
    for index, mutation, argument in (
        (-1, {"repeated": False}, "probe"),
        (-4, {"kind": "string"}, "sensor"),
        (-2, {"kind": "string"}, "waiting"),
    ):
        bad_cli = json.loads((root / "dynamic-cli.json").read_bytes())
        bad_cli["root"]["arguments"][index].update(mutation)
        write("bad-dynamic-cli.json", bad_cli)
        dynamic_package["components"][0]["graph"] = pin("bad-dynamic-cli.json")
        write("dynamic-package.json", dynamic_package)
        bad = subprocess.run([ODDLANG, "build", "native-package", "rust", root / "dynamic-package.json",
                              root / "bad-dynamic-client"], capture_output=True, timeout=120)
        assert bad.returncode != 0 and f'incompatible argument "{argument}"'.encode() in bad.stderr
        assert not (root / "bad-dynamic-client").exists()

    typed = json.loads((root / "graph.json").read_bytes())
    typed["transport"]["maximum_frame_bytes"] = 512
    typed["response"] = json.loads((ROOT / "typed-response.json").read_bytes())
    write("typed.json", typed)
    manifest["components"][1]["graph"] = pin("typed.json")
    write("manifest.json", manifest)
    checked(ODDLANG, "build", "native-package", "rust", root / "manifest.json", root / "typed-client")
    typed_bin = field_meter(root / "typed-client")
    exchange(typed_bin, path, reply(b'[null,true]'), (0, b'{"reading":null,"ready":true}\n', b""))
    exchange(typed_bin, path, reply(b'[null,false,"stale"]'), (3, b'{"reading":null,"ready":false,"notice":"stale"}\n', b""))
    exchange(typed_bin, path, reply(b'{"ready":true,"reading":{"mode":"idle","discard":0}}'),
             (0, b'{"reading":{"mode":"idle"},"ready":true}\n', b""))
    exchange(typed_bin, path, reply(b'{"ready":true,"reading":{"rows":[{"reference":[1,2],"value":255}],"mode":"scan","digest":"face"}}'),
             (0, b'{"reading":{"mode":"scan","digest":"face","rows":[{"value":255,"reference":[1,2]}]},"ready":true}\n', b""))
    for body in (b'[null]', b'[null,true,null,0]', b'{"ready":true,"reading":{"mode":"other"}}',
                 b'{"ready":true,"reading":{"mode":"scan","digest":"FACE","rows":[]}}',
                 b'{"ready":true,"reading":{"mode":"scan","digest":"face","rows":[],"extra":1}}',
                 b'{"ready":true,"reading":{"mode":"scan","digest":"face","rows":[{"value":256,"reference":[1,2]}]}}',
                 b'{"ready":true,"reading":{"mode":"scan","digest":"face","rows":[{"value":255,"reference":[1]}]}}'):
        exchange(typed_bin, path, reply(body), (10, b"", b"invalid sensor response\n"))
    manifest["components"][1]["graph"] = pin("graph.json")

    discovery_graph = json.loads((root / "graph.json").read_bytes())
    endpoint_cli = json.loads((root / "cli.json").read_bytes())
    endpoint_cli["root"]["arguments"].extend([
        {"id":"folder", "long":"folder", "kind":"path", "global":True},
        {"id":"station", "long":"station", "kind":"string", "global":True},
    ])

    def diagnostic(stderr):
        return {"exit":8, "stderr":stderr}

    def endpoint_field(owner, key, kind):
        return {"id":f"urn:meter:{owner}:{key}", "key":key, "type":{"kind":kind}}

    def operand(kind, **keys):
        return {"kind":kind, **keys}

    discovery_graph["discovery"] = {
        "id":"urn:meter:endpoint", "state_argument":"folder", "profile_argument":"station",
        "default_profile":"bench", "socket_leaf":"sock", "socket_maximum_exclusive":104,
        "state_sources":[{"variable":"METER_STORE", "require_absolute":True, "suffix":["meter"]}],
        "missing_state":diagnostic("missing meter store"),
        "profile_atom":{"maximum_bytes":10, "punctuation":"-", "invalid":diagnostic("invalid station")},
        "private_directory_mode":448, "private_file_mode":384,
        "unsafe_directory":diagnostic("unsafe station directory"), "unsafe_file":diagnostic("unsafe station file"),
        "invalid_runtime_component":diagnostic("invalid component"), "socket_too_long":diagnostic("long path"),
        "profile":{"id":"urn:meter:station", "leaf":"station.json", "maximum_bytes":128, "overflow":"reject",
                   "invalid":diagnostic("invalid station document"), "missing":diagnostic("missing station document"),
                   "fields":[endpoint_field("station","slot","text")], "checks":[
                       {"id":"urn:meter:slot-hex","require":{"kind":"lower_hex", "value":operand("profile_field",id="urn:meter:station:slot"),
                        "bytes":2,"nullable":False},"rejected":diagnostic("invalid station slot")},
                   ]},
        "runtime":[operand("literal",value="meter-"), operand("effective_uid"),operand("literal",value="-"),
                   operand("profile_field",id="urn:meter:station:slot")],
        "binding":{"id":"urn:meter:link", "leaf":"link.json", "maximum_bytes":128, "overflow":"reject",
                   "invalid":diagnostic("invalid link"), "fields":[endpoint_field("link","path","text")],
                   "checks":[{"id":"urn:meter:link-owner","require":{"kind":"equal","left":operand("binding_field",id="urn:meter:link:path"),
                               "right":operand("profile_path")},"rejected":diagnostic("incorrect station binding")}]},
    }
    write("discovery.json", discovery_graph)
    write("discovery-cli.json", endpoint_cli)
    checked(ODDLANG, "build", "local-command-client", "rust", root / "discovery.json",
            root / "binding.json", root / "effects")
    catalog = (root / "effects/endpoint-obligations.json").read_bytes()
    assert json.loads((root / "effects/projection.lock").read_bytes())["endpointObligationsSha256"] == hashlib.sha256(catalog).hexdigest()
    manifest["components"][0]["graph"] = pin("discovery-cli.json")
    manifest["components"][1]["graph"] = pin("discovery.json")
    write("manifest.json", manifest)
    checked(ODDLANG, "build", "native-package", "rust", root / "manifest.json", root / "discovery-client")
    discovery_bin = field_meter(root / "discovery-client")
    home, scratch = root / "h", root / "t"
    state = home / "meter"
    station = state / "bench"
    runtime = scratch / f"meter-{os.geteuid()}-1a2b"
    for directory in (home, state, station, scratch, runtime):
        directory.mkdir(mode=0o700)
    env = {"PATH":os.defpath, "TMPDIR":str(scratch), "METER_STORE":str(home)}
    station_file, binding = station / "station.json", runtime / "link.json"
    station_file.write_text('{"slot":"1a2b"}')
    station_file.chmod(0o600)
    binding.write_text(json.dumps({"path":str(station.resolve())}))
    binding.chmod(0o600)
    positive = (0, b'{"reading":null,"ready":true}\n', b"")
    exchange(discovery_bin, runtime / "sock", reply(b'{"ready":true}'), positive, args=["read"], env=env)

    def declined(message):
        case = subprocess.run([discovery_bin, "read"], env=env, capture_output=True, timeout=5)
        assert (case.returncode, case.stdout, case.stderr) == (8, b"", message + b"\n")

    binding.write_text('{"path":"/elsewhere"}')
    declined(b"incorrect station binding")
    station_file.chmod(0o644)
    declined(b"unsafe station file")
    env["METER_STORE"] = "relative"
    declined(b"missing meter store")
    exchange(discovery_bin, path, reply(b'{"ready":true}'), positive,
             args=["read","--folder",state,"--station","bad/name","--port",path], env=env)
    for name, update, diagnostic_text in (
        ("escape", lambda policy: policy.update(socket_leaf="../sock"), b"invalid local endpoint discovery policy"),
        ("future", lambda policy: policy["profile"]["checks"][0]["require"].update(
            value=operand("binding_field",id="urn:meter:link:path")), b"runtime binding field used before reading it"),
    ):
        invalid_graph = json.loads((root / "discovery.json").read_bytes())
        update(invalid_graph["discovery"])
        write(name + ".json", invalid_graph)
        invalid = subprocess.run([ODDLANG, "build", "local-command-client", "rust", root / (name + ".json"),
                                  root / "binding.json", root / name], capture_output=True)
        assert invalid.returncode and diagnostic_text in invalid.stderr and not (root / name).exists()
    manifest["components"][0]["graph"] = pin("cli.json")
    manifest["components"][1]["graph"] = pin("graph.json")

    def reject_link(message, output):
        write("manifest.json", manifest)
        invalid = subprocess.run(
            [ODDLANG, "build", "native-package", "rust", root / "manifest.json", root / output],
            capture_output=True,
        )
        assert invalid.returncode and message in invalid.stderr, invalid.stderr
        assert not (root / output).exists()

    graph = json.loads((root / "graph.json").read_bytes())
    graph["guards"][0]["argument"] = "units"
    write("unknown-guard.json", graph)
    manifest["components"][1]["graph"] = pin("unknown-guard.json")
    reject_link(b"needs undeclared root argument", "bad-guard")

    graph["guards"][0]["argument"] = "port"
    write("wrong-guard.json", graph)
    manifest["components"][1]["graph"] = pin("wrong-guard.json")
    reject_link(b"has incompatible argument", "bad-kind")

    graph["guards"][0]["argument"] = "unit"
    manifest["components"][1]["graph"] = pin("graph.json")
    cli = json.loads((root / "cli.json").read_bytes())
    unit = cli["root"]["arguments"][1]
    unit["global"] = False
    cli["root"]["subcommands"][0]["arguments"] = [{**unit, "id": "shadow_unit"}]
    write("shadowed.json", cli)
    manifest["components"][0]["graph"] = pin("shadowed.json")
    reject_link(b"has incompatible argument", "shadowed-guard")
    manifest["components"][0]["graph"] = pin("cli.json")

    unlinked = json.loads((root / "cli-rust.json").read_bytes())
    unlinked["handlers"]["urn:field:read"] = ["device", "other"]
    write("unlinked.json", unlinked)
    manifest["components"][0]["overlay"] = pin("unlinked.json")
    reject_link(b"unresolved checked native command handler", "bad-handler")

    graph["transport"]["maximum_frame_bytes"] = 1
    write("mutant.json", graph)
    mutant = subprocess.run([ODDLANG, "build", "local-command-client", "rust", root / "mutant.json",
                             root / "binding.json", root / "invalid"], capture_output=True)
    assert mutant.returncode and b"constant request exceeds frame limit" in mutant.stderr
    assert not (root / "invalid").exists()

print("field-meter IPC: literal and typed request frames, frame limits, argument checks, typed replies, and private endpoint discovery passed")
