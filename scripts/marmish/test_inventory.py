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

    def test_platform_predicates_separate_ignore_and_eligibility(self):
        from inventory_platforms import eligible

        attrs = ['#[cfg(all(test,target_os="linux"))]', '#[ignore = "host gate"]']
        self.assertIs(eligible(attrs, {"test", 'target_os="linux"'}), True)
        self.assertIs(eligible(attrs, {"test", 'target_os="macos"'}), False)
        self.assertIsNone(eligible(["#[cfg(undocumented_build_script_cfg)"], {"test"}))
        self.assertIs(eligible(["#[cfg_attr(test, track_caller)]"], {"test"}), True)

    def test_union_and_per_target_units_stay_distinct(self):
        from inventory_platforms import denominators

        s = {
            "id": "A",
            "kind": "function_item",
            "classification": "retained-native",
            "targets": ["a", "b"],
        }
        info = denominators(
            [s], {"crate.rs": {"parseStatus": "recovered"}}, ["host"], ["default"]
        )
        self.assertEqual(info["lexicalUnion"]["observedSites"], 1)
        self.assertEqual(info["lexicalUnion"]["byCargoTarget"]["a"]["function_item"], 1)
        self.assertIsNone(info["compiledRuntimeUnits"]["host"]["default"]["count"])

    def test_checked_ignored_and_package_inputs_follow_native_matrix(self):
        import json
        from pathlib import Path

        summary = json.loads(
            (
                Path(__file__).resolve().parents[2]
                / "examples/marmish/inventory-summary.json"
            ).read_text()
        )
        for target, platform in summary["platformInventory"].items():
            apple = target == "aarch64-apple-darwin"
            ignored = platform["ignoredSites"]
            self.assertTrue(any("native_macos.rs" in path for path in ignored) == apple)
            self.assertTrue(any("native_linux.rs" in path for path in ignored) != apple)
            package = platform["packaging"]
            self.assertEqual(len(package["recipes"]), 2)
            for recipe in package["recipes"]:
                self.assertEqual(recipe["target"], target)
                self.assertEqual(recipe["signing"], "adhoc" if apple else "none")
                self.assertEqual(
                    recipe["workflowSha256"],
                    package["trackedRecipeInputs"][recipe["workflow"]],
                )
            self.assertEqual(
                {f["path"] for f in package["finalInputs"]},
                {
                    "package-input/marmish",
                    "package-input/marmishd",
                    "package-input/marmish-worker",
                    "THIRD-PARTY-LICENSES",
                },
            )
            self.assertTrue(all(f["sha256"] is None for f in package["finalInputs"]))

    def test_collect_platform_tests_preserves_unknown_and_ignored_gates(self):
        from inventory_platforms import test_inventory

        path = "crates/fixture/src/lib.rs"
        target = "fixture:lib:fixture"
        sites = [
            (2, "apple", ['#[cfg(target_os = "macos")]']),
            (4, "linux", ['#[cfg(target_os = "linux")]']),
            (6, "custom", ["#[cfg(compiler_supplied_switch)]"]),
            (8, "manual", ['#[ignore = "explicit gate"]']),
        ]
        tests = [
            {"line": line, "name": name, "attributes": ["#[test]", *attrs]}
            for line, name, attrs in sites
        ]
        syntax = {
            path: {
                "tests": tests,
                "attributes": [[line, line, attrs] for line, _, attrs in sites],
            }
        }
        files = {path: {"cargoContexts": [{"target": target, "attributes": []}]}}
        platforms = {
            "aarch64-apple-darwin": ['target_os="macos"'],
            "x86_64-unknown-linux-gnu": ['target_os="linux"'],
        }
        catalog, collected = test_inventory(
            syntax, [], files, {target: {"crate": "fixture"}}, platforms, ["default"]
        )
        for platform, selected in [
            ("aarch64-apple-darwin", "apple"),
            ("x86_64-unknown-linux-gnu", "linux"),
        ]:
            with self.subTest(platform=platform):
                rows = {
                    catalog[r["test"]]["name"]: (
                        r["gate"],
                        r["profiles"],
                        r["uncertainProfiles"],
                    )
                    for r in collected[platform]
                }
                self.assertEqual(
                    rows,
                    {
                        selected: ("ordinary", ["default"], []),
                        "custom": ("ordinary", [], ["default"]),
                        "manual": ("ignored", ["default"], []),
                    },
                )
                self.assertTrue(
                    all(r["cargoTarget"] == target for r in collected[platform])
                )
