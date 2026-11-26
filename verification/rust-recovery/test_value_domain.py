"""Recover an unrelated scalar domain and falsify the interval hypothesis with a hole."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
LIB = """
#[derive(Debug)]
pub enum Alarm { Outside, Broken }
impl std::fmt::Display for Alarm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Alarm::Outside => "sensor outside interval",
            Alarm::Broken => "sensor broken",
        })
    }
}
pub struct Sensor(Option<u8>);
impl Sensor {
    pub const CEILING: u8 = if cfg!(oddlang_injected) { 200 } else { MAXIMUM };
    pub fn sample(n: Option<u8>) -> Result<Self, Alarm> {
        if n.is_some_and(|v| !(3..=Self::CEILING).contains(&v) HOLE) { Err(Alarm::Outside) } else { Ok(Self(n)) }
    }
    pub fn reading(&self) -> Option<u8> { self.0 }
}
impl Default for Sensor { fn default() -> Self { Self(Some(5)) } }
"""


def run(*args, cwd=None, ok=True, env=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True, env=env)
    if (result.returncode == 0) != ok:
        raise RuntimeError(result.stderr)
    return result


with tempfile.TemporaryDirectory() as directory:
    base = Path(directory)
    root = base / "crate"
    (root / "src").mkdir(parents=True)
    (root / "Cargo.toml").write_text('[package]\nname="sensor"\nversion="0.1.0"\nedition="2024"')
    (root / "src/lib.rs").touch()
    (root / ".gitignore").write_text("/.cargo/\n")
    run("git", "init", "-q", cwd=root)
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)
    for maximum, hole in [(11,False),(13,False),(13,True)]:
        (root/"src/lib.rs").write_text(LIB.replace("MAXIMUM",str(maximum)).replace("HOLE","|| v == 9" if hole else ""))
        run("git","add",".",cwd=root)
        run("git","-c","user.name=fixture","-c","user.email=fixture@invalid.example","commit","-qm","sensor",cwd=root)
        pin = run("git","rev-parse","HEAD",cwd=root).stdout.strip()
        selection = dict(package="sensor", bits=8,rangeError="urn:s:outside",sourceRevision=pin,
                         cargoLockSha256=hashlib.sha256((root/"Cargo.lock").read_bytes()).hexdigest(),
                         compilerCommit="8bab26f4f68e0e26f0bb7960be334d5b520ea452",
                         api=dict(domain="urn:s:pressure",typeName="Sensor",constructor="sample",accessor="reading",
                                  maximumConstant="CEILING",errorType="Alarm",
                                  variants={"urn:s:outside":"Outside", "urn:s:broken":"Broken"},
                                  variantOrder=["urn:s:outside", "urn:s:broken"]))
        path=base/"selection.json"
        path.write_text(json.dumps(selection))
        output=base/pin
        run(CLI,"recover","value-domain",root/"Cargo.toml",path,output,ok=not hole)
        if hole:
            assert not output.exists()
        else:
            ttl=(output/"domain.ttl").read_text()
            assert f"c:minimum 3 ; c:maximum {maximum} ; c:defaultValue 5" in ttl
            assert json.loads((output/"observation.json").read_bytes())["numericCoverage"]=="exhaustive"

            if maximum == 13:
                wrong_order = {**selection, "api": {
                    **selection["api"], "variantOrder": ["urn:s:broken", "urn:s:outside"]
                }}
                altered = base / "wrong-order.json"
                destination = base / "unexpected-order"
                altered.write_text(json.dumps(wrong_order))
                failed = run(CLI, "recover", "value-domain", root / "Cargo.toml", altered, destination, ok=False)
                assert "unexpected enum ordinal" in failed.stderr
                assert not destination.exists()
                (root / ".git/info/attributes").write_text("src/lib.rs export-ignore\n")
                for cargo_config in (root / ".cargo", base / ".cargo", base / "user-cargo"):
                    cargo_config.mkdir()
                    (cargo_config / "config.toml").write_text(
                        '[build]\nrustflags = ["--cfg", "oddlang_injected"]\n'
                    )
                wrapper = base / "compiler-wrapper"
                marker = base / "wrapper-ran"
                wrapper.write_text(f'#!/bin/sh\ntouch "{marker}"\nexec "$@"\n')
                wrapper.chmod(0o700)
                env = {
                    **os.environ,
                    "CARGO_HOME": str(base / "user-cargo"),
                    "CARGO_BUILD_RUSTC": str(wrapper),
                    "CARGO_BUILD_RUSTC_WRAPPER": str(wrapper),
                    "RUSTC_WORKSPACE_WRAPPER": str(wrapper),
                    "CARGO_ENCODED_RUSTFLAGS": "--cfg\x1foddlang_injected",
                }
                isolated = base / "isolated"
                run(CLI, "recover", "value-domain", root / "Cargo.toml", path, isolated, env=env)
                assert not marker.exists(), "inherited wrapper executed during recovery"
                for name in ("domain.ttl", "observation.json"):
                    assert (isolated / name).read_bytes() == (output / name).read_bytes(), name
                probe = json.loads((isolated / "observation.json").read_text())["probeSha256"]
                assert len(probe) == 64 and all(c in "0123456789abcdef" for c in probe)
    print("two revisions recovered; hidden config and wrappers isolated; noninterval source and wrong enum order rejected")
