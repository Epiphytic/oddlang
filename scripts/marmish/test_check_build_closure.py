import tempfile
import unittest
from pathlib import Path

from check_build_closure import FRAME, NATIVE_SCRIPT, check_depinfo, dep_paths, sha, stage_candidate


class BuildClosureTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.src = self.root / "src"
        self.target = self.root / "target"
        self.target.mkdir()
        rows = []
        for rel in [FRAME, NATIVE_SCRIPT, "Cargo.lock"]:
            path = self.src / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture")
            path.chmod(0o644)
            rows.append({"path": rel, "sha256": sha(b"fixture"), "mode": "100644",
                         "producer": "generated" if rel == FRAME else "retained"})
        self.manifest = {"cargoLockSha256": sha(b"fixture"), "files": rows}

    def test_undeclared_file_and_symlink_fail_before_build(self):
        extra = self.src / "undeclared"
        for linked in (False, True):
            if linked:
                extra.symlink_to(self.src / FRAME)
            else:
                extra.write_text("unexpected")
            with self.subTest(linked=linked), self.assertRaisesRegex(ValueError, "undeclared or special"):
                stage_candidate(self.src, self.manifest, self.root / "copy")
            extra.unlink()

    def test_compiler_must_observe_generated_and_retained_sources(self):
        dep = self.target / "unit.d"
        dep.write_text("unit: " + FRAME + "\n")
        entries = {FRAME, NATIVE_SCRIPT}
        def inspect():
            return check_depinfo(self.src, self.target, self.root / "vendor", self.root / "rust",
                                 self.root / "sdk", entries)
        with self.assertRaisesRegex(ValueError, "native build.rs"):
            inspect()
        dep.write_text("unit: " + FRAME + " " + NATIVE_SCRIPT + "\n")
        self.assertEqual(inspect()[1], entries)
        dep.write_text("unit: " + FRAME + " " + str(self.root / "unlisted") + "\n")
        with self.assertRaisesRegex(ValueError, "escaped declared roots"):
            inspect()

    def test_rustc_make_escapes(self):
        dep = self.target / "unit.d"
        dep.write_text("target: some\\ file.rs file.rs\n\n# env-dep:X=abc\n")
        self.assertEqual(dep_paths(dep), ["some file.rs", "file.rs"])
