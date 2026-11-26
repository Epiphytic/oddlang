#!/usr/bin/env python3
"""Exercise the actual compiler adapter on a small standalone Cargo workspace."""

import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import unittest


RUN = Path(__file__).with_name("run.sh")
LIBRARY = """
#![allow(dead_code, unexpected_cfgs)]
macro_rules! declare { () => { fn generated() -> u8 { 2 } }; }
declare!();

mod helpers {
    pub fn first() -> u8 { 1 }
    pub fn second() -> u8 { 1 }
    pub fn a(n: u8) -> u8 { n }
    pub fn b(n: u8) -> u8 { n }
    fn private() -> u8 { 9 }
}
pub use helpers::first as alias;

pub trait Source { fn get(&self) -> u8; }
struct Item { part: u8 }
struct Outer { item: Item }
struct OnDrop;
impl Drop for OnDrop {
    fn drop(&mut self) { std::hint::black_box(0); }
}

fn scope() { let _held = OnDrop; }
fn nested(value: &Outer) -> u8 { value.item.part }
fn inline_const() -> u8 { const { 4 } }
enum Discriminant { Variant = 5 }
fn pick(values: Vec<u8>) -> Vec<u8> {
    values.into_iter().map(helpers::a).collect()
}

#[cfg(any(unpinned_rustflag, unpinned_encoded, debug_assertions))]
fn extra_unpinned() {}
impl Source for Item {
    fn get(&self) -> u8 { self.part }
}

fn private(value: &mut Item) -> u8 {
    value.part = helpers::first() + renamed::combine(generated());
    value.part
}

#[cfg(feature = "extra")]
pub fn dynamic(value: &dyn Source) -> u8 { value.get() }

fn unused_private() -> u8 { 3 }

pub fn concrete() -> u8 {
    let mut item = Item { part: 0 };
    private(&mut item)
}
"""


class AdapterTests(unittest.TestCase):
    def test_compiler_resolves_declarations_and_edges(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            dependency = root / "actual"
            dependency.mkdir()
            (root / "src").mkdir()
            (dependency / "src").mkdir()
            (root / "Cargo.toml").write_text("""
[package]
name = "sample"
version = "0.1.0"
edition = "2024"

[dependencies]
renamed = { path = "actual", package = "actual" }

[features]
extra = []
""")
            (dependency / "Cargo.toml").write_text("""
[package]
name = "actual"
version = "0.1.0"
edition = "2024"
""")
            (dependency / "src/lib.rs").write_text("pub fn combine(a: u8) -> u8 { a }")
            (root / "src/lib.rs").write_text(LIBRARY)
            (root / "tests").mkdir()
            (root / "tests/public.rs").write_text("""
#[test]
fn unchanged_consumer() {
    assert_eq!(sample::alias(), 1);
    assert_eq!(sample::concrete(), 3);
}
""")
            subprocess.run(
                ["cargo", "+1.97.1", "generate-lockfile", "--offline"],
                cwd=root,
                check=True,
                capture_output=True,
            )
            target = subprocess.check_output(
                ["rustc", "+1.97.1", "-vV"], text=True
            ).split("host: ")[1].splitlines()[0]
            self.assertTrue(RUN.exists(), "compiler adapter entry point is missing")

            def cargo(*args):
                return subprocess.run(
                    ["cargo", "+1.97.1", args[0], "--locked", "--offline", *args[1:]],
                    cwd=root, check=True, capture_output=True,
                )

            def mir_calls():
                cargo("rustc", "--lib", "--features", "extra", "--", "--emit=mir")
                outputs = list((root / "target/debug/deps").glob("sample-*.mir"))
                self.assertEqual(len(outputs), 1)
                mir = outputs[0].read_text()
                body = re.search(r"^fn private\(_1: &mut Item\).*?^}", mir, re.M | re.S)
                self.assertIsNotNone(body)
                return set(re.findall(r"= ([A-Za-z_][A-Za-z_0-9]*)\(.*?\) -> \[.*?return:", body[0]))

            def inventory(source, *features, env=None):
                (root / "src/lib.rs").write_text(source)
                return json.loads(subprocess.check_output([
                    RUN, root / "Cargo.toml", "sample", "lib", "sample",
                    target, "no-default", *features,
                ], env=env))

            report = inventory(LIBRARY, "extra")
            cargo("test", "--test", "public")
            self.assertEqual(mir_calls(), {"first", "combine", "generated"})
            declarations = {
                row["name"]: row for row in report["declarations"]
                if row["name"] in {
                    "generated", "first", "second", "part", "dynamic"
                } or row["id"] == "local::sample::private"
            }
            edges = {
                (row["from"], row["relation"], row["to"])
                for row in report["edges"]
            }
            private = declarations["private"]["id"]
            self.assertEqual(
                {row["id"] for row in report["declarations"] if row["name"] == "private"},
                {private, "local::sample::helpers::{type}::private"},
            )
            reformatted = inventory(LIBRARY.replace("fn private(value", "\n\nfn private(value"), "extra")
            self.assertEqual(
                {row["id"] for row in report["declarations"]},
                {row["id"] for row in reformatted["declarations"]},
            )
            self.assertEqual(report["edges"], reformatted["edges"])
            self.assertEqual(report["gaps"], reformatted["gaps"])
            self.assertEqual(declarations["private"]["visibility"], "private")
            self.assertTrue(declarations["generated"]["macroGenerated"])
            alias = next(row for row in report["declarations"] if row["name"] == "alias")
            self.assertIn((alias["id"], "reexport", declarations["first"]["id"]), edges)
            self.assertEqual(
                declarations["private"]["signature"]["output"],
                {"kind": "primitive", "name": "u8"},
            )
            self.assertIn((private, "call", declarations["first"]["id"]), edges)
            self.assertNotIn((private, "call", declarations["second"]["id"]), edges)
            self.assertIn((private, "call", "external::actual::combine"), edges)
            self.assertIn((private, "read", declarations["part"]["id"]), edges)
            self.assertIn((private, "write", declarations["part"]["id"]), edges)
            self.assertIn(
                ("local::sample::pick", "function-reference", "local::sample::helpers::{type}::a"),
                edges,
            )
            for owner, kind in [
                ("local::sample::scope", "implicit-drop"),
                ("local::sample::nested", "nested-field-projection"),
                ("local::sample::inline_const", "skipped-body"),
                ("local::sample::Discriminant::{type}::Variant::{type}", "skipped-body"),
            ]:
                self.assertIn({"owner": owner, "kind": kind}, report["gaps"])
            self.assertNotIn("extra_unpinned", {row["name"] for row in report["declarations"]})
            polluted_env = os.environ | {
                "RUSTFLAGS": "--cfg unpinned_rustflag",
                "CARGO_ENCODED_RUSTFLAGS": "--cfg\x1funpinned_encoded",
                "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS": "true",
            }
            self.assertEqual(report, inventory(LIBRARY, "extra", env=polluted_env))
            self.assertIn(
                {"owner": declarations["dynamic"]["id"], "kind": "dynamic-dispatch"},
                report["gaps"],
            )

            variant = LIBRARY.replace("helpers::first() +", "helpers::second() +")
            variant = variant.replace("fn unused_private() -> u8 { 3 }", "")
            variant = variant.replace("map(helpers::a)", "map(helpers::b)")
            other = inventory(variant)
            cargo("test", "--test", "public")
            self.assertEqual(mir_calls(), {"second", "combine", "generated"})
            original_ids = {row["id"] for row in report["declarations"]}
            changed_ids = {row["id"] for row in other["declarations"]}
            self.assertEqual(
                original_ids - changed_ids,
                {
                    "local::sample::dynamic",
                    "local::sample::dynamic::'_::{lifetime}",
                    "local::sample::unused_private",
                },
            )
            other_edges = {
                (row["from"], row["relation"], row["to"])
                for row in other["edges"]
            }
            self.assertNotIn((private, "call", declarations["first"]["id"]), other_edges)
            self.assertIn((private, "call", declarations["second"]["id"]), other_edges)
            self.assertNotIn(
                ("local::sample::pick", "function-reference", "local::sample::helpers::{type}::a"),
                other_edges,
            )
            self.assertIn(
                ("local::sample::pick", "function-reference", "local::sample::helpers::{type}::b"),
                other_edges,
            )


if __name__ == "__main__":
    unittest.main()
