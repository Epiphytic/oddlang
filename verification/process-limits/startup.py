"""Exercise real generated entry wrappers and sibling dependency validation."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ODDLANG = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, CARGO_NET_OFFLINE="true", RUSTUP_TOOLCHAIN="1.97.1")


def run(*args, cwd, check=True):
    return subprocess.run(args, cwd=cwd, env=ENV, check=check, capture_output=True, timeout=120)


def pin(path):
    return dict(path=path.name, sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def write(root, name, obj):
    path = root / name
    path.write_text(json.dumps(obj))
    return pin(path)


def seed(root, name, dependency=None, clap=False):
    member = root / name
    (member / "src").mkdir(parents=True)
    (member / "src/lib.rs").touch()
    dependencies = {"starter-lib": '{ path = "../starter-lib" }'} if dependency else {}
    if clap:
        dependencies["clap"] = '"=4.6.7"'
    (member / "Cargo.toml").write_text(f'[package]\nname="{name}"\n'
        'version="0.1.0"\nedition="2021"\n[dependencies]\n' +
        "".join(f"{key}={value}\n" for key,value in dependencies.items()))
    return member


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    seed(root, "starter-lib")
    seed(root, "field-meter", dependency=True, clap=True)
    (root/"Cargo.toml").write_text('[workspace]\nresolver="2"\n'
        'members=["starter-lib","field-meter"]\n')
    run("cargo", "generate-lockfile", "--offline", cwd=root)
    graph = write(root, "graph.json", dict(format="oddlang-process-limit-v1",
        id="urn:meter:initialize", resource="open-descriptors",
        platforms=dict(linux=dict(kind="no-op"), macos=dict(kind="no-op")),
        failure="urn:meter:unavailable"))
    overlay = write(root, "rust.json", dict(entry="initialize", error={
        "id": "urn:meter:unavailable", "type": "LimitError", "variant": "Unavailable",
        "result": "Result", "display": "unavailable"}))
    library = dict(format="oddlang-native-package-v1", name="starter-lib", version="0.1.0",
        edition="2021", cargoLock=pin(root/"Cargo.lock"), components=[dict(kind="process-limit",
        module="limits", reexports=["initialize"], graph=graph, overlay=overlay)])
    cli = write(root, "cli.json", dict(profile="command-surface-v1", package_name="field-meter",
        version="0.1.0", root=dict(name="field-meter", help="Read a field meter")))
    rust = write(root, "cli-rust.json", dict(executable="field-meter", invalid_exit=2,
        uncovered_exit=125, operations={}))
    start = dict(dependency="starter-lib", entry=["initialize"], exit=4, stderr="start rejected")
    product = dict(format="oddlang-native-package-v1", name="field-meter", version="0.1.0",
        edition="2021", cargoLock=pin(root/"Cargo.lock"), dependencies=dict(clap=dict(version="4.6.7")),
        components=[dict(kind="cli-surface", module="cli", reexports=["run_cli"], graph=cli, overlay=rust)],
        binaries=[dict(name="field-meter", entry=["run_cli"], startup=start)])
    # The per-package locks reflect their standalone closure.
    solo = root/"solo"
    solo.mkdir()
    for member, clap in ((library, False), (product, True)):
        standalone = seed(solo, member["name"], clap=clap)
        with (standalone/"Cargo.toml").open("a") as manifest:
            manifest.write("\n[workspace]\n")
        run("cargo", "generate-lockfile", "--offline", cwd=standalone)
        lock = root/(member["name"]+".lock")
        lock.write_bytes((standalone/"Cargo.lock").read_bytes())
        member["cargoLock"] = pin(lock)
    members = {"starter-lib": dict(manifest=write(root,"library.json",library)),
               "field-meter": dict(dependencies=["starter-lib"])}
    workspace = dict(format="oddlang-native-workspace-v1", cargoLock=pin(root/"Cargo.lock"), members=members)

    def build(name, error=None):
        members["field-meter"]["manifest"] = write(root, "product.json", product)
        manifest = root/write(root, "workspace.json", workspace)["path"]
        output = root/name
        got = run(ODDLANG, "build", "native-workspace", "rust", manifest, output,
                  cwd=root, check=False)
        if error:
            assert got.returncode and error in got.stderr.decode() and not output.exists(), got
        else:
            assert not got.returncode, got.stderr.decode()
        return output

    output = build("product")
    run("cargo", "build", "--locked", "--offline", "--bins", cwd=output)
    binary = output/"target/debug/field-meter"
    got = run(binary, "--help", cwd=root)
    assert b"Read a field meter" in got.stdout and not got.stderr, got
    wrapper = output/"field-meter/src/bin/field-meter.rs"
    assert b"::starter_lib::initialize" in wrapper.read_bytes()
    # Fail the typed operation in a test-only replacement for the provider. The
    # emitted wrapper stays byte-identical, as does the CLI entrypoint.
    leaf = output/"starter-lib/src/group_0/__projection_0.rs"
    with leaf.open("w") as provider:
        provider.write("pub fn initialize() -> ::std::result::Result<(), ()> { Err(()) }\n")
    run("cargo", "build", "--locked", "--offline", "--bins", cwd=output)
    got = run(binary, "--bad-option", cwd=root, check=False)
    assert (got.returncode,got.stdout,got.stderr) == (4,b"",b"start rejected\n"), got
    members["field-meter"]["dependencies"] = []
    build("no-edge", "startup dependency")
    members["field-meter"]["dependencies"] = ["starter-lib"]
    start["entry"] = ["absent"]
    build("no-function", "invalid workspace binary entry binding")
    print("Sibling startup succeeds before CLI and emits its declared failure before argument parsing")
