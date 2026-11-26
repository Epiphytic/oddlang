"""Build a source-free resource policy and check actual changes in a child."""
import hashlib
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1", CARGO_NET_OFFLINE="true")


def run(*args, cwd, check=True, preexec_fn=None):
    return subprocess.run(args, cwd=cwd, env=ENV, check=check,
                          capture_output=True, timeout=120, preexec_fn=preexec_fn)


def pin(path):
    return {"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def write(root, name, value):
    path = root / name
    path.write_text(json.dumps(value))
    return pin(path)


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    (root / "src").mkdir()
    (root / "src/lib.rs").touch()
    deps = {"libc": {"version": "0.2.189"}}
    (root / "Cargo.toml").write_text('[package]\nname="water-monitor"\n'
        'version="0.1.0"\nedition="2021"\n[dependencies]\nlibc="=0.2.189"\n')
    run("cargo", "generate-lockfile", "--offline", cwd=root)
    graph = {
        "format": "oddlang-process-limit-v1",
        "id": "urn:water:descriptor-budget",
        "resource": "open-descriptors",
        "platforms": {"linux": {"kind": "no-op"},
                      "macos": {"kind": "lower-soft", "ceiling": 384}},
        "failure": "urn:water:budget-unavailable",
    }
    overlay = {"entry": "initialize_budget", "error": {
        "type": "BudgetError", "variant": "Unavailable", "result": "Result",
        "id": graph["failure"], "display": "budget unavailable",
    }}
    component = dict(kind="process-limit", module="limits",
        reexports=["initialize_budget", "BudgetError", "Result"],
        graph=write(root, "graph.json", graph), overlay=write(root, "rust.json", overlay))
    package = dict(format="oddlang-native-package-v1", name="water-monitor",
        version="0.1.0", edition="2021", cargoLock=pin(root/"Cargo.lock"),
        dependencies=deps, components=[component])
    product = root/"product"
    manifest = root/write(root, "package.json", package)["path"]
    build = run(CLI, "build", "native-package", "rust", manifest, product,
                cwd=root, check=False)
    assert build.returncode == 0, build.stderr.decode()
    (product/"src/bin").mkdir()
    (product/"src/bin/inspect.rs").write_text("""
use std::os::fd::AsRawFd;
fn limits() -> (libc::rlim_t, libc::rlim_t) {
    let mut value = libc::rlimit {rlim_cur: 0, rlim_max: 0};
    assert_eq!(unsafe {libc::getrlimit(libc::RLIMIT_NOFILE, &mut value)}, 0);
    (value.rlim_cur, value.rlim_max)
}
fn main() {
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd = file.as_raw_fd();
    let flags = unsafe {libc::fcntl(fd, libc::F_GETFD)};
    assert!(flags >= 0);
    let before = limits();
    let initialize: fn() -> water_monitor::Result<()> = water_monitor::initialize_budget;
    initialize().unwrap();
    let first = limits();
    initialize().unwrap();
    assert_eq!(limits(), first);
    assert_eq!(first.1, before.1);
    assert_eq!(unsafe {libc::fcntl(fd, libc::F_GETFD)}, flags);
    println!("{} {}", before.0, first.0);
    assert_eq!(water_monitor::BudgetError::Unavailable.to_string(), "budget unavailable");
}
""")
    run("cargo", "build", "--locked", "--offline", "--bin", "inspect", cwd=product)
    hard = resource.getrlimit(resource.RLIMIT_NOFILE)[1]
    for soft in (383, 384, 385):
        def set_soft():
            resource.setrlimit(resource.RLIMIT_NOFILE, (soft, hard))
        got = run(product/"target/debug/inspect", cwd=root, preexec_fn=set_soft)
        expected = min(soft, 384) if sys.platform == "darwin" else soft
        assert got.stdout == f"{soft} {expected}\n".encode() and not got.stderr, got

    def rejected(error):
        write(root, "package.json", package)
        invalid = root/"invalid"
        got = run(CLI, "build", "native-package", "rust", manifest, invalid,
                  cwd=root, check=False)
        assert got.returncode and error in got.stderr.decode() and not invalid.exists(), got

    graph["platforms"]["macos"]["ceiling"] = 0
    component["graph"] = write(root, "graph.json", graph)
    rejected("ceiling")
    graph["platforms"]["macos"]["ceiling"] = 384
    component["graph"] = write(root, "graph.json", graph)
    overlay["error"]["id"] = "urn:water:other"
    component["overlay"] = write(root, "rust.json", overlay)
    rejected("failure identity")
    print("Portable descriptor budget: low/equal/high child cases and invalid graph/binding pass")
