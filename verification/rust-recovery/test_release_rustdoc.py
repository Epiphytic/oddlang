"""Exercise independent observations and mutations under a real release profile."""
import copy
import tempfile
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import release_rustdoc as check


class ReleaseComparison(unittest.TestCase):
    def test_profile_identity_and_mutations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            manifest = root / "Cargo.toml"
            manifest.write_text('[package]\nname="specimen"\nversion="1.0.0"\nedition="2024"\n'
                                '[features]\nchoose=[]\n[profile.release]\ndebug-assertions=true\n')
            source = root / "src/lib.rs"
            source.write_text("""
mod first { fn read() {} }
mod second { fn read() {} }
pub struct Sample { pub tag: u8 }
#[cfg(feature="choose")] pub fn selected() {}
#[cfg(debug_assertions)] pub fn asserts_enabled() {}
#[cfg(doc)] pub fn only_docs() {}
#[cfg(not(doc))] pub fn only_compiler() {}
""")
            subprocess.run(["cargo", "+1.97.1", "generate-lockfile", "--offline"], cwd=root, check=True)
            target = check.tool_identity()["host"]
            options = (root, "specimen", "lib", "specimen", target, False, ["choose"])
            with patch.object(check, "compare", wraps=check.compare) as capture:
                results = check.collect(*options)
                original_compiler = capture.call_args.args[0]
            compared = results["comparison"]

            def names(group):
                return {tuple(entry["path"]) for entry in compared[group]}

            matches = names("matched")
            self.assertTrue({("specimen", "first", "read"), ("specimen", "second", "read"),
                             ("specimen", "Sample", "tag"), ("specimen", "selected"),
                             ("specimen", "asserts_enabled")} <= matches)
            self.assertEqual(names("compilerOnly"), {("specimen", "only_compiler")})
            self.assertEqual(names("rustdocOnly"), {("specimen", "only_docs")})
            doc_profile = copy.deepcopy(results["profile"])
            doc_profile["explicitCfg"].append('feature="wrong"')
            with self.assertRaisesRegex(ValueError, "explicitCfg"):
                check.compare_profiles(results["profile"], doc_profile)
            before = names("matched")
            source.write_text(source.read_text().replace("pub tag", "pub renamed"))
            with patch.object(check, "compare", wraps=check.compare) as capture:
                compared = check.collect(*options)["comparison"]
                changed_document = capture.call_args.args[1]
            self.assertEqual(before - names("matched"), {("specimen", "Sample", "tag")})
            self.assertEqual(names("matched") - before, {("specimen", "Sample", "renamed")})
            compared = check.compare(original_compiler, changed_document, root)
            self.assertIn(("specimen", "Sample", "tag"), names("compilerOnly"))
            self.assertIn(("specimen", "Sample", "renamed"), names("rustdocOnly"))


if __name__ == "__main__":
    unittest.main()
