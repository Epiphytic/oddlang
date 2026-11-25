import unittest
from inventory import classify_file, reference_scopes, require_complete


class InventoryTests(unittest.TestCase):
    def test_classification_is_closed(self):
        self.assertEqual(classify_file("scripts/install.sh"), "generated-artifact")
        self.assertEqual(classify_file("crates/marmish-native/build.rs"), "build-input")
        self.assertEqual(
            classify_file("tests/fixtures/relay-canonicalization.json"), "test-fixture"
        )
        with self.assertRaises(ValueError):
            classify_file("unexpected/runtime.xyz")

    def test_scope_ordinals_are_local_and_one_based(self):
        ids = {"a.rs": ["a/f", "a/g"], "b.rs": ["b/f"]}
        self.assertEqual(
            reference_scopes("b.rs", [[0, "use", "T", 1], [1, "call", "f", 2]], ids),
            [None, "b/f"],
        )
        with self.assertRaises(ValueError):
            reference_scopes("b.rs", [[2, "call", "g", 2]], ids)

    def test_open_reachability_rejects_completeness(self):
        with self.assertRaisesRegex(ValueError, "reachable"):
            require_complete(["reachable callees remain unresolved"])

    def test_cfg_test_helpers_and_platform_modules_follow_cargo_root(self):
        from inventory import classify_symbol, module_contexts

        targets = {
            "c:lib:c": {
                "kind": "lib",
                "name": "c",
                "path": "src/lib.rs",
                "requiredFeatures": [],
            }
        }
        files = {
            p: {} for p in ("src/lib.rs", "src/system.rs", "src/platform/apple.rs")
        }
        syntax = {
            "src/lib.rs": {
                "edges": [
                    {
                        "kind": "mod",
                        "path": None,
                        "name": "system",
                        "inline": [],
                        "line": 4,
                    }
                ],
                "attributes": [[4, 4, ["#[cfg(test)]"]]],
            },
            "src/system.rs": {
                "edges": [
                    {
                        "kind": "mod",
                        "path": "platform/apple.rs",
                        "name": "platform",
                        "inline": [],
                        "line": 2,
                    }
                ],
                "attributes": [[2, 2, ['#[cfg(target_os = "macos")]']]],
            },
        }
        contexts, missing = module_contexts(files, targets, syntax)
        self.assertFalse(missing)
        context = contexts["src/platform/apple.rs"]
        self.assertEqual(
            set(context[0]["attributes"]),
            {"#[cfg(test)]", '#[cfg(target_os = "macos")]'},
        )
        self.assertEqual(
            classify_symbol(
                "crates/c/src/platform/apple.rs", "cargo-source", context, [], targets
            )[0],
            "test",
        )
        del files["src/platform/apple.rs"]
        contexts, missing = module_contexts(files, targets, syntax)
        self.assertEqual(missing, [("src/system.rs", 2, "c:lib:c")])
