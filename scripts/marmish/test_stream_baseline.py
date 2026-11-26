"""Regression controls for the stream baseline receipt."""
import copy
from pathlib import Path
import subprocess
import tempfile
import unittest

from stream_baseline import (
    compare, compiler_inventory, dep_info, inline_test_span, observed_path,
    tracked_sources,
)


class StreamBaselineTests(unittest.TestCase):
    def test_depinfo_preserves_escaped_paths_and_environment(self):
        paths, env = dep_info(
            "output.d: src/main.rs src/a\\ b.rs \\\n src/c.rs\n"
            "# env-dep:ABSENT\n# env-dep:VALUE=x=y\n"
        )
        self.assertEqual(paths, ["src/main.rs", "src/a b.rs", "src/c.rs"])
        self.assertEqual(env, {"ABSENT": None, "VALUE": "x=y"})

    def test_an_item_after_the_inline_test_module_is_excluded(self):
        module = b"#[cfg(test)]\nmod tests {\n    fn first() {}\n}"
        self.assertEqual(inline_test_span(module, "fixture.rs"),
                         inline_test_span(module + b"\nfn following() {\n}\n", "fixture.rs"))
        self.assertEqual(inline_test_span(module, "fixture.rs")["endByte"], len(module))

    def test_extra_unreferenced_package_rust_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            path = "crates/marmish-stream/src/lib.rs"
            (root / path).parent.mkdir(parents=True)
            (root / path).write_text("fn f() {}\n")
            (root / path).with_name("unexpected.rs").write_text("")
            with self.assertRaisesRegex(ValueError, "extra .*unexpected.rs"):
                tracked_sources(root, {path: "0" * 40})

    def test_unpaired_depinfo_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            workspace, target, toolchain = (root / name for name in ("src", "target", "compiler"))
            workspace.mkdir()
            deps = target / "debug/deps"
            deps.mkdir(parents=True)
            (workspace / "foo.rs").write_text("")
            (deps / "foo.d").write_text("foo.d: foo.rs\n")
            (deps / "orphan.d").write_text("orphan.d: foo.rs\n")
            command = f" Running `{toolchain}/bin/rustc --crate-name foo foo.rs --out-dir {deps}`"
            with self.assertRaisesRegex(ValueError, "unpaired rustc dep-info.*orphan.d"):
                compiler_inventory(command, workspace, target, root / "cargo", None, toolchain, root / "sdk",
                                   {"foo.rs": "ignored"}, {})

    def test_real_rustc_include_bytes_canary_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            source = root / "lib.rs"
            source.write_text('pub const BYTES: &[u8] = include_bytes!("undeclared.txt");\n')
            (root / "undeclared.txt").write_text("compiler input canary")
            out = root / "lib.d"
            subprocess.run(
                ["rustc", "+1.97.1", "--crate-type", "lib", "--emit", f"dep-info={out}", source],
                cwd=root, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            paths, _ = dep_info(out.read_text())
            undeclared = next(path for path in paths if path.endswith("/undeclared.txt"))
            with self.assertRaisesRegex(ValueError, "undeclared compiler input undeclared.txt"):
                observed_path(undeclared, root, root / "target", root / "cargo", None, root / "toolchain",
                              root / "sdk", {"lib.rs": "declared"}, {})

    def test_same_root_and_mutated_inputs_are_rejected(self):
        fields = ("rustc", "cargo", "target", "targetCfg", "sourceRemapping", "cargoInputs",
                  "trackedSources", "inlineTestSpans")
        original = {field: "frozen" for field in fields}
        original.update({"workspaceRoot": "upstream", "role": "original", "builds": {}})
        candidate = dict(original, role="candidate")
        with self.assertRaisesRegex(ValueError, "candidate is the original workspace"):
            compare(original, candidate)
        candidate["workspaceRoot"] = "candidate"
        compare(original, candidate)
        changed = copy.deepcopy(candidate)
        changed["trackedSources"] = "changed"
        with self.assertRaisesRegex(ValueError, "baseline mismatch: trackedSources"):
            compare(original, changed)


if __name__ == "__main__":
    unittest.main()
