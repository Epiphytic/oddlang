"""Verify observed public names with actual dependent crates and a specialized impl."""

from pathlib import Path
import subprocess
import tempfile
import unittest

import release_rustdoc as check


class InherentNames(unittest.TestCase):
    def test_public_names_have_concrete_receivers(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text('[package]\nname="optic"\nversion="0.1.0"\nedition="2024"\n')
            source = root / "src/lib.rs"
            source.write_text("""
pub mod sensor {
    pub struct Meter;
    impl Meter { pub const LIMIT: u8 = 3; pub fn read(&self) {} fn hidden() {} }
    #[doc(hidden)] impl Meter { #[doc(hidden)] pub fn hidden_from_docs() {} }
    #[cfg(not(doc))] impl Meter { pub fn compiler_only() {} }
    pub trait Measure { fn derived(); }
    impl Measure for Meter { fn derived() {} }
    pub struct Batch<T>(pub T);
    impl Batch<u8> { pub fn specific(&self) {} }
}
""")
            subprocess.run(["cargo", "+1.97.1", "generate-lockfile", "--offline"], cwd=root, check=True)
            report = check.collect(root, "optic", "lib", "optic", check.tool_identity()["host"],
                                   False, [], public_inherent=True)["rustdocPublicInherent"]
            owner = ["optic", "sensor", "Meter"]
            self.assertEqual(report["items"], [{"owner": owner, "kind": "assoc_const", "name": "LIMIT"},
                                               {"owner": owner, "kind": "function", "name": "hidden_from_docs"},
                                               {"owner": owner, "kind": "function", "name": "read"}])
            self.assertEqual(report["excludedIndexedLocalInherentImpls"]["generic-or-specialized-receiver"], 1)
            rustc = check.tool("rustc")
            lib = root / "liboptic.rlib"
            subprocess.run([rustc, "--crate-type=rlib", "--crate-name", "optic", source, "-o", lib], check=True)
            for expression, diagnostic in [
                ("let _ = Meter::LIMIT; Meter.read(); Meter::hidden_from_docs(); Meter::compiler_only(); Batch(1u8).specific();", None),
                ("Meter::hidden();", "E0624"), ("Batch(1u16).specific();", "E0599"),
            ]:
                caller = root / "caller.rs"
                caller.write_text("use optic::sensor::{Batch, Meter}; fn main() {" + expression + "}")
                result = subprocess.run([rustc, "--edition=2024", "--emit=metadata", "--extern",
                                         f"optic={lib}", caller, "-o", root / "caller.rmeta"], capture_output=True)
                if diagnostic:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(diagnostic.encode(), result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
