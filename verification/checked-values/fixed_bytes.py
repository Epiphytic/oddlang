"""Recover two real array extents, then compare matching Rust public observations."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
ROOT = Path(__file__).resolve().parents[2]
VALUE = "urn:sample:calibration"
FAILURE = "urn:sample:width-error"
SELECTION = dict(domain="urn:sample:calibration-domain", value=VALUE, failure=FAILURE,
                 source="local::instrument::Calibration::{type}")
BINDING = dict(format="oddlang-checked-values-rust-v1", variants={},
               errors={FAILURE: dict(errorType="crate::Fault", variant="Width")},
               types={VALUE: dict(name="Calibration", order=[], constructor="take", parse="read", view="sample")})
MANIFEST = '[package]\nname="instrument"\nversion="1.0.0"\nedition="2021"\n[dependencies]\nzeroize="=1.9.1"\n'
REFERENCE = """
pub enum Fault { Width }
pub struct Calibration(zeroize::Zeroizing<[u8; WIDTH]>);
impl Calibration {
    pub fn take(value: [u8; WIDTH]) -> Self { Self(zeroize::Zeroizing::new(value)) }
    pub fn read(bytes: &[u8]) -> Result<Self, Fault> {
        let mut fixed = [0; WIDTH];
        if fixed.len() != bytes.len() { return Err(Fault::Width); }
        fixed.copy_from_slice(bytes);
        Ok(Self::take(fixed))
    }
    pub fn sample(&self) -> &[u8; WIDTH] { &self.0 }
}
"""
CALLER = """
use instrument::{Calibration, Fault};
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::{Zeroize,Zeroizing};
fn main() {
    assert!(std::mem::needs_drop::<Calibration>());
    let _borrow: fn(&Calibration) -> &[u8; WIDTH] = Calibration::sample;
    let mut input = [11; WIDTH];
    let owned = Calibration::read(&input).ok().unwrap();
    input.fill(99);
    assert_eq!(owned.sample(), &[11; WIDTH]);
    assert_eq!(Calibration::take(input).sample(), &[99; WIDTH]);
    for n in 0..=12 {
        match Calibration::read(&vec![3;n]) {
            Ok(value) => println!("{n} {:?}", value.sample()),
            Err(Fault::Width) => println!("{n} rejected"),
        }
    }
    // Observe the provider's drop hook without reading freed/uninitialized storage.
    struct Sentinel<'a>(&'a AtomicBool);
    impl Zeroize for Sentinel<'_> { fn zeroize(&mut self) { self.0.store(true, Ordering::Relaxed); } }
    let erased = AtomicBool::new(false);
    drop(Zeroizing::new(Sentinel(&erased)));
    assert!(erased.load(Ordering::Relaxed));
    input.zeroize();
    assert_eq!(input, [0; WIDTH]);
}
"""


def run(*command, cwd, check=True, **kwargs):
    return subprocess.run(command, cwd=cwd, capture_output=True, check=check, **kwargs)


def write(path, data):
    path.write_text(json.dumps(data))


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    host = next(v[6:] for v in run("rustc", "+1.97.1", "-vV", cwd=root).stdout.decode().splitlines()
                if v.startswith("host: "))
    write(root / "selection.json", SELECTION)
    write(root / "overlay.json", BINDING)
    for width in (7, 9):
        crate = root / str(width)
        (crate / "src").mkdir(parents=True)
        (crate / "Cargo.toml").write_text(MANIFEST)
        source = crate / "src/lib.rs"
        source.write_text(REFERENCE.replace("WIDTH", str(width)))
        main = crate / "src/main.rs"
        main.write_text(CALLER.replace("WIDTH", str(width)))
        run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=crate)
        observed = run("bash", ROOT / "verification/rust-recovery/run.sh", crate / "Cargo.toml",
                       "instrument", "lib", "instrument", host, "default", cwd=crate,
                       env=dict(os.environ, ODD_RECOVERY_DATA_SHAPES="1")).stdout
        (crate / "observed.json").write_bytes(observed)
        run(CLI, "recover", "fixed-bytes", "observed.json", root / "selection.json", "recovered", cwd=crate)
        graph = crate / "recovered/graph.json"
        assert json.loads(graph.read_bytes())["types"][VALUE] == {
            "kind":"fixed-bytes", "length":width, "onDrop":"erase", "failure":FAILURE}
        lock = json.loads((crate / "recovered/recovery.lock").read_bytes())
        assert lock["observed"]["length"] == width and "failure" in lock["authored"]
        bad = json.loads(observed)
        bad["crateName"] = "different"
        write(crate / "wrong.json", bad)
        rejected = run(CLI, "recover", "fixed-bytes", "wrong.json", root / "selection.json", "invalid", cwd=crate, check=False)
        assert rejected.returncode and b"compiler or crate" in rejected.stderr and not (crate/"invalid").exists()
        run(CLI, "build", "checked-values", "rust", graph, root / "overlay.json", "product", cwd=crate)
        cargo = ("cargo", "+1.97.1", "run", "--quiet", "--offline", "--locked")
        original = run(*cargo, cwd=crate).stdout
        source.write_text('pub enum Fault { Width }\nmod projected; pub use projected::Calibration;\n')
        (crate / "src/projected.rs").write_bytes((crate / "product/values.rs").read_bytes())
        actual = run(*cargo, cwd=crate).stdout
        assert actual == original and len(actual.splitlines()) == 13
        assert actual.splitlines()[width].endswith(str([3]*width).encode())
        for trait in ("Clone", "Copy", "std::fmt::Debug", "PartialEq"):
            main.write_text(f"fn must<T: {trait}>() {{}} fn main() {{must::<instrument::Calibration>();}}")
            failed = run("cargo", "+1.97.1", "check", "--offline", "--locked", "--message-format=json",
                         cwd=crate, check=False)
            codes = {d["message"]["code"]["code"] for line in failed.stdout.splitlines()
                     if (d:=json.loads(line)).get("reason")=="compiler-message" and d["message"]["code"]}
            assert failed.returncode and "E0277" in codes, (trait,codes)
    print("Recovered 7 and 9 octets; both native bounds, copy ownership, borrowing, drop provider and trait controls passed.")

# Exercise the package admission checks before Cargo sees any untrusted source URL.
from verify import FAULT, pin, shared
with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    graph, error_binding = shared(root)
    fixed = dict(format="oddlang-checked-values-v1", id=SELECTION["domain"],
                 types={VALUE:dict(kind="fixed-bytes",length=7,onDrop="erase",failure=FAULT)})
    binding = dict(BINDING, errors={FAULT:dict(errorType="crate::Fault",variant="Invalid")})
    write(root/"fixed.json", fixed)
    write(root/"overlay.json", binding)
    (root/"Cargo.lock").write_text('version=4\n[[package]]\nname="instrument"\nversion="1.0.0"\n')
    pkg = dict(format="oddlang-native-package-v1",name="instrument",version="1.0.0",edition="2021",
               cargoLock=pin(root/"Cargo.lock",root), components=[
                   dict(kind="shared-declarations",module="faults",graph=pin(graph,root),
                        overlay=pin(error_binding,root),reexports=["Fault"]),
                   dict(kind="checked-values",module="fixed",graph=pin(root/"fixed.json",root),
                        overlay=pin(root/"overlay.json",root),reexports=["Calibration"])])
    git = dict(git="https://example.org/provider", revision="1"*40)
    for dependency, patch, message in [
        ({}, {}, "locked registry provider"),
        ({"version":"1.8.0"}, {}, "locked registry provider"),
        ({"version":"1.9.1","source":git},{}, "locked registry provider"),
        ({"version":"1.9.1"},{"zeroize":git},"locked registry provider"),
        ({"version":"1.9.1"},{},"expected locked registry source and checksum")]:
        write(root/"package.json", dict(pkg, dependencies={"zeroize":dependency} if dependency else {}, registryPatches=patch))
        r = run(CLI,"build","native-package","rust",root/"package.json",root/"rejected",cwd=root,check=False)
        assert r.returncode and message.encode() in r.stderr and not (root/"rejected").exists(),r.stderr
