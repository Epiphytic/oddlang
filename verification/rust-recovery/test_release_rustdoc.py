"""Exercise independent observations and mutations under a real release profile."""
import copy
import tempfile
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import release_rustdoc as check


class ReleaseComparison(unittest.TestCase):
    def test_missing_and_ambiguous_producers_reject_the_observation(self):
        package = {Path("/src/reading"): {"name": "reading", "version": "1.0", "path": "reading"}}
        invocation = ("  Running `CARGO_MANIFEST_DIR=/src/reading /tool/rustc --crate-name reading "
                      "--crate-type lib --out-dir /build -C extra-filename=-abc`\n")
        for log, count in [("", 0), (invocation * 2, 2)]:
            with self.subTest(count=count), self.assertRaisesRegex(ValueError, f"found {count}"):
                check.extern_bindings(log, Path("/tool/rustc"), ["--extern", "value=/build/libreading-abc.rmeta"], package)

    def test_unselected_host_producer_needs_no_target_package_entry(self):
        invocation = ("  Running `CARGO_MANIFEST_DIR=/src/hostonly /tool/rustc --crate-name hostonly "
                      "--crate-type dylib --out-dir /host`\n")
        self.assertEqual(check.extern_bindings(invocation, Path("/tool/rustc"), [], {}), {})
        with self.assertRaisesRegex(ValueError, "unknown Cargo package producing extern"):
            check.extern_bindings(invocation, Path("/tool/rustc"),
                                  ["--extern", "alias=/host/libhostonly.dylib"], {})

    def test_cross_target_with_host_only_build_dependency(self):
        target = "wasm32-wasip1"
        installed = subprocess.check_output(
            ["rustup", "target", "list", "--installed", "--toolchain", check.TOOLS], text=True).splitlines()
        if target not in installed:
            self.skipTest(f"toolchain lacks {target}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "hostonly/src").mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[package]\nname="cross-observer"\nversion="1.0.0"\nedition="2024"\n'
                '[target.\'cfg(not(target_family="wasm"))\'.build-dependencies]\nhostonly={path="hostonly"}\n')
            (root / "hostonly/Cargo.toml").write_text('[package]\nname="hostonly"\nversion="1.0.0"\nedition="2024"\n')
            (root / "hostonly/src/lib.rs").write_text("pub fn metadata() {}\n")
            (root / "build.rs").write_text("fn main() { hostonly::metadata(); }\n")
            (root / "src/lib.rs").write_text("pub struct Reading;\n")
            subprocess.run(["cargo", "+1.97.1", "generate-lockfile", "--offline"], cwd=root, check=True)
            report = check.collect(root, "cross-observer", "lib", "cross_observer", target, False, [])
            self.assertEqual(report["profile"]["externBindings"], {})
            matched = [r["path"] for r in report["comparison"]["matched"]]
            self.assertIn(["cross_observer", "Reading"], matched)

    def test_declared_access_survives_spelling_and_detects_restriction_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text('[package]\nname="access-sample"\nversion="1.0.0"\nedition="2024"\n')
            source = root / "src/lib.rs"
            source.write_text("""
fn local() {}
mod outer {
    pub mod inner {
        pub struct Sample { hidden: u8, pub(super) limit: u8, pub(crate) count: u8, pub value: u8 }
        pub(in crate::outer) fn observe() {}
        pub enum State { Ready }
    }
}
""")
            subprocess.run(["cargo", "+1.97.1", "generate-lockfile", "--offline"], cwd=root, check=True)
            options = (root, "access-sample", "lib", "access_sample", check.tool_identity()["host"], False, [])
            with patch.object(check, "compare", wraps=check.compare) as capture:
                before = check.collect(*options)["comparison"]["declaredAccess"]
                original, doc, _ = capture.call_args.args
            self.assertEqual(before["different"], [])
            self.assertEqual(before["unresolved"], [])
            rows = {row["path"][-1]: row["compiler"] for row in before["agreement"]}
            self.assertEqual(rows["local"], {"within": ["access_sample"]})
            self.assertEqual(rows["hidden"], {"within": ["access_sample", "outer", "inner"]})
            self.assertEqual(rows["limit"], {"within": ["access_sample", "outer"]})
            self.assertEqual(rows["observe"], rows["limit"])
            self.assertEqual(rows["count"], rows["local"])
            self.assertEqual(rows["value"], "public")
            self.assertNotIn("Ready", rows)

            # A missing compiler restriction must stay unknown even when rustdoc has a scope.
            missing = copy.deepcopy(original)
            field = next(row["id"] for row in missing["declarations"] if row["name"] == "limit")
            missing["edges"] = [e for e in missing["edges"] if (e["from"], e["relation"]) != (field, "visibility-scope")]
            unknown = check.compare(missing, doc, root)["declaredAccess"]["unresolved"]
            self.assertEqual([row["path"][-1] for row in unknown], ["limit"])

            source.write_text(source.read_text().replace("pub(super) limit", "pub(crate) limit"))
            with patch.object(check, "compare", wraps=check.compare) as capture:
                after = check.collect(*options)["comparison"]["declaredAccess"]
                changed_doc = capture.call_args.args[1]
            self.assertEqual(after["different"], [])
            self.assertEqual(after["unresolved"], [])
            changed = check.compare(original, changed_doc, root)
            difference, = changed["declaredAccess"]["different"]
            self.assertEqual(difference["path"][-1], "limit")
            self.assertEqual(difference["compiler"], rows["limit"])
            self.assertEqual(difference["rustdoc"], rows["local"])
            self.assertIn(difference["path"], [row["path"] for row in changed["matched"]])

    def test_profile_identity_and_mutations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            manifest = root / "Cargo.toml"
            manifest.write_text('[package]\nname="specimen"\nversion="1.0.0"\nedition="2024"\n'
                                '[features]\nchoose=["dep:first_read"]\n[profile.release]\ndebug-assertions=true\n')
            for path, version in (("old", "1.0.0"), ("new", "2.0.0")):
                dep = root / path
                (dep / "src").mkdir(parents=True)
                (dep / "Cargo.toml").write_text(f'[package]\nname="reading"\nversion="{version}"\nedition="2024"\n')
                if path == "new":
                    with (dep / "Cargo.toml").open("a") as out:
                        out.write('[lib]\ncrate-type=["dylib"]\n')
                (dep / "src/lib.rs").write_text("pub struct Reading;\n")
            with manifest.open("a") as out:
                out.write('[dependencies]\nserde_derive="=1.0.229"\nfirst_read={package="reading",path="old",optional=true}\n'
                          'next_read={package="reading",path="new"}\n')
            source = root / "src/lib.rs"
            source.write_text("""
mod first { fn read() {} }
mod second { fn read() {} }
pub struct Sample { pub tag: u8 }
#[cfg(feature="choose")] pub type Prior = first_read::Reading;
pub type Next = next_read::Reading;
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
            bindings = results["profile"]["externBindings"]
            self.assertEqual(bindings["first_read"]["package"], {"name": "reading", "version": "1.0.0", "path": "old"})
            self.assertEqual(bindings["next_read"]["package"], {"name": "reading", "version": "2.0.0", "path": "new"})
            self.assertEqual(bindings["next_read"]["crateTypes"], ["dylib"])
            proc_macro = bindings["serde_derive"]
            self.assertEqual(proc_macro["crateTypes"], ["proc-macro"])
            self.assertEqual(proc_macro["target"], [])
            self.assertEqual(proc_macro["package"]["version"], "1.0.229")
            self.assertTrue(proc_macro["package"]["source"].startswith("registry+"))
            swapped = copy.deepcopy(results["profile"])
            swapped["externBindings"]["first_read"] = bindings["next_read"]
            with self.assertRaisesRegex(ValueError, "externBindings"):
                check.compare_profiles(results["profile"], swapped)
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
                text = manifest.read_text().replace('path="old",optional=true', 'path="new",optional=true')
                manifest.write_text(text.replace('next_read={package="reading",path="new"', 'next_read={package="reading",path="old"'))
                changed = check.collect(*options)
                self.assertEqual(changed["profile"]["externAliases"], results["profile"]["externAliases"])
                self.assertEqual(changed["profile"]["externBindings"]["first_read"], bindings["next_read"])
                with self.assertRaisesRegex(ValueError, "externBindings"):
                    check.compare_profiles(results["profile"], changed["profile"])
                compared = changed["comparison"]
                changed_document = capture.call_args.args[1]
            self.assertEqual(before - names("matched"), {("specimen", "Sample", "tag")})
            self.assertEqual(names("matched") - before, {("specimen", "Sample", "renamed")})
            compared = check.compare(original_compiler, changed_document, root)
            self.assertIn(("specimen", "Sample", "tag"), names("compilerOnly"))
            self.assertIn(("specimen", "Sample", "renamed"), names("rustdocOnly"))


if __name__ == "__main__":
    unittest.main()
