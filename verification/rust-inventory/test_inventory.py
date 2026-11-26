import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from inventory import bind_bundle, input_identity, package_id


class InventoryTests(unittest.TestCase):
    def test_registry_and_versioned_vendor_have_the_same_identity(self):
        cache = Path("/cargo/registry/src")
        vendor = Path("/workspace-vendor")
        relative = "some-library-0.1.0/src/lib.rs"
        roots = [("registry-cache", cache), ("registry", vendor)]
        self.assertEqual(
            input_identity(cache / "index-root" / relative, roots, {}),
            input_identity(vendor / relative, roots, {}),
        )

    def test_package_identity_survives_workspace_relocation(self):
        for root in (Path("/original"), Path("/emitted")):
            self.assertEqual(package_id(f"path+{root.as_uri()}#probe@0.1.0", root), "workspace:#probe@0.1.0")
            self.assertEqual(
                package_id(f"path+{root.as_uri()}/crates/probe#0.1.0", root),
                "workspace:crates/probe#0.1.0",
            )

    def test_a_stale_bundle_cannot_authorize_an_old_source_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            source = base / "lib.rs"
            source.write_bytes(b"const PRESENT: u32 = 1;")
            (base / "syntax.ttl").write_bytes(b"<urn:test> <urn:value> \"PRESENT\" .\n")
            (base / "sources.json").write_text(json.dumps({"files": {"lib.rs": digest(source)}}))
            lock = base / "bundle.lock"
            lock.write_text(json.dumps({"version": 1, "syntaxSha256": digest(base / "syntax.ttl"),
                                        "sourcesSha256": digest(base / "sources.json")}))
            pin = digest(lock)
            self.assertEqual(bind_bundle(base, pin, base)["sourcesMatched"], 1)
            source.write_bytes(b"const PRESENT: u32 = 2;")
            with self.assertRaisesRegex(ValueError, "stale syntax bundle source digest: lib.rs"):
                bind_bundle(base, pin, base)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    unittest.main()
