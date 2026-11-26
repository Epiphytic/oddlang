"""Verify locked dependency acquisition against a disposable local Git provider."""
import hashlib
import json
import os
from pathlib import Path
import subprocess


def verify(cli, root, make, pin, put):
    root.mkdir()
    provider = root / "provider"
    provider.mkdir()
    (provider / "src").mkdir()
    (provider / "Cargo.toml").write_text('[package]\nname="reading"\nversion="0.1.0"\nedition="2021"\n')
    (provider / "src/lib.rs").write_text("pub const MAX: u8 = 9;\n")
    def git(*args):
        return subprocess.check_output(["git", "-C", str(provider), *args], text=True).strip()
    git("init", "--quiet")
    git("add", "src", "Cargo.toml")
    git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
        "-c", "commit.gpgSign=false", "commit", "--quiet", "-m", "fixture")
    revision = git("rev-parse", "HEAD")
    url = "https://fixture.invalid/reading"
    files = root / "inputs"
    files.mkdir()
    make(files, "instrument", "LIMIT", 9)
    path = files / "instrument-package.json"
    package = json.loads(path.read_bytes())
    package["dependencies"]["reading"] = {"version": "0.1.0", "source": {"git": url, "revision": revision}}
    lock = files / "instrument.lock"
    lock.write_text(f'''version = 4
[[package]]
name = "instrument"
version = "0.1.0"
dependencies = ["reading"]
[[package]]
name = "reading"
version = "0.1.0"
source = "git+{url}?rev={revision}#{revision}"
''')
    package["cargoLock"] = pin(lock)
    workspace = {"format": "oddlang-native-workspace-v1", "cargoLock": pin(lock),
                 "members": {"instrument": {"manifest": put(path, package)}}}
    spec = files / "workspace.json"
    put(spec, workspace)
    env = dict(os.environ, CARGO_HOME=str(root / "cache"), CARGO_NET_GIT_FETCH_WITH_CLI="true",
               GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0=f"url.{provider.as_uri()}.insteadOf",
               GIT_CONFIG_VALUE_0=url, GIT_TERMINAL_PROMPT="0")
    def build(name, *flags):
        return subprocess.run([cli, "build", "native-workspace", "rust", spec, root / name, *flags],
                              cwd=root, env=env, capture_output=True, text=True)
    cold = build("cold")
    assert cold.returncode != 0 and "offline" in cold.stderr and not (root / "cold").exists(), cold.stderr
    acquired = build("acquired", "--fetch-locked")
    assert acquired.returncode == 0, acquired.stderr
    provider.rename(root / "provider-disconnected")
    offline = build("offline")
    assert offline.returncode == 0, offline.stderr
    def inventory(name):
        base = root / name
        return {p.relative_to(base): hashlib.sha256(p.read_bytes()).digest()
                for p in base.rglob("*") if p.is_file()}
    assert inventory("acquired") == inventory("offline")
    assert (root / "offline/Cargo.lock").read_bytes() == lock.read_bytes()
    # A graph-repinned, stale workspace lock must still fail in bootstrap mode.
    (root / "provider-disconnected").rename(provider)
    stale = files / "stale.lock"
    stale.write_text('version=4\n[[package]]\nname="instrument"\nversion="0.1.0"\n')
    workspace["cargoLock"] = pin(stale)
    put(spec, workspace)
    rejected = build("rejected", "--fetch-locked")
    assert rejected.returncode != 0 and "locked" in rejected.stderr and not (root / "rejected").exists(), rejected.stderr
