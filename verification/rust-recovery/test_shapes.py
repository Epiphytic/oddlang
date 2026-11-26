#!/usr/bin/env python3
"""Exercise portable data shapes through Oddlang on unrelated Cargo sources."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
BINARY = Path(sys.argv.pop(1)).resolve()
SOURCE = """
#![allow(dead_code)]
use std::sync::Arc;

struct Frame<'a, T> {
    word: u16,
    shared: Arc<T>,
    map: Choice<T>,
    read: &'a str,
    write: &'a mut T,
    mixed: (i8, bool),
    array: [u8; 3],
    #[cfg(feature = "expanded")]
    extra: f32,
}
enum Choice<T> {
    Zero,
    One(T, u128),
    More { raw: *const T },
    EmptyTuple(),
    EmptyRecord {},
}
struct Packet(u32, i64);
struct Marker;
union Bits { left: u32, right: i32 }
"""


NETWORK = """
#![allow(dead_code)]
struct Arc<T>(T);
struct Window<'a, T, const N: usize>(&'a T, [T; N]);
struct Route<'a> {
    alias: Arc<u8>,
    cache: std::rc::Rc<u32>,
    next: Box<Option<Route<'a>>>,
    bytes: Window<'a, u16, 8>,
    endpoint: &'static str,
    offset: usize,
}
macro_rules! status { () => { enum Status { Ok = 1, Offline } }; }
status!();
"""


class Shapes(unittest.TestCase):
    def test_release_compiler_shape_graph(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            host = subprocess.check_output(["rustc", "+1.97.1", "-vV"], text=True)
            host = host.split("host: ")[1].splitlines()[0]
            manifest = root / "Cargo.toml"
            manifest.write_text("""
[package]
name = "observatory"
version = "0.1.0"
edition = "2024"

[features]
expanded = []
""")
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text(SOURCE)
            subprocess.run(
                ["cargo", "+1.97.1", "generate-lockfile", "--offline"],
                cwd=root, check=True,
            )
            digest = hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest()
            profile = root / "profile.toml"
            base = f"""
adapter = "{HERE / 'run.sh'}"
adapterInputs = ["Cargo.toml", "Cargo.lock", "driver.rs", "shapes.rs", "shape_model.rs"]
package = "observatory"
targetKind = "lib"
targetName = "observatory"
targetTriple = "{host}"
defaultFeatures = false
compilerCommit = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
cargoLockSha256 = "{digest}"
"""

            def extract(source, name, feature=False):
                profile.write_text(base + ('features = ["expanded"]\n' if feature else ""))
                (root / "src/lib.rs").write_text(source)
                output = root / name
                result = subprocess.run(
                    [BINARY, "recover", "rust-shapes", manifest, "--profile", profile,
                     "--out", output], cwd=root, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                lock = json.loads((output / "shapes.lock").read_bytes())
                graph = (output / "shapes.json").read_bytes()
                self.assertEqual(lock["shapesSha256"], hashlib.sha256(graph).hexdigest())
                self.assertEqual(lock["qualification"], "candidate")
                return graph, {item["name"]: item for item in json.loads(graph)["declarations"]}

            original, rows = extract(SOURCE, "original")
            self.assertEqual(set(rows), {"Bits", "Choice", "Frame", "Packet", "Marker"})
            frame = rows["Frame"]
            fields = {field["name"]: field for field in frame["fields"]}
            self.assertEqual(list(fields), ["word", "shared", "map", "read", "write", "mixed", "array"])
            self.assertEqual(fields["word"]["type"], {"kind": "unsigned-integer", "bits": 16})
            self.assertEqual(fields["shared"]["type"]["ownership"], "shared")
            args = fields["map"]["type"]["arguments"]
            self.assertEqual([arg["name"] for arg in args], ["T"])
            self.assertEqual(args[0]["value"]["id"], frame["parameters"][1]["id"])
            self.assertEqual(fields["read"]["type"]["ownership"], "shared-borrow")
            self.assertEqual(fields["write"]["type"]["ownership"], "exclusive-borrow")
            self.assertEqual(fields["mixed"]["type"]["elements"][1]["position"], 1)
            self.assertEqual(fields["array"]["type"], {"kind": "array", "extent": {"kind": "fixed", "count": 3},
                                                      "element": {"kind": "unsigned-integer", "bits": 8}})
            variants = rows["Choice"]["variants"]
            self.assertEqual([v["name"] for v in variants], ["Zero", "One", "More", "EmptyTuple", "EmptyRecord"])
            self.assertEqual([v["form"] for v in variants], ["unit", "tuple", "record", "tuple", "record"])
            self.assertEqual([f["position"] for f in variants[1]["fields"]], [0, 1])
            self.assertEqual(variants[1]["fields"][0]["owner"], variants[1]["id"])
            self.assertEqual(rows["Packet"]["fields"][1]["type"], {"kind": "signed-integer", "bits": 64})
            self.assertEqual(rows["Bits"]["kind"], "union")
            self.assertNotIn(
                {"owner": fields["array"]["id"], "reason": "array-length"},
                json.loads(original)["gaps"],
            )

            # Replay a pinned older adapter observation and reject the same
            # vocabulary label when a nested field actually uses v2 arrays.
            fake = root / "legacy.py"
            payload = root / "legacy.json"
            fake.write_text("#!/usr/bin/env python3\nfrom pathlib import Path\nprint(Path(__file__).with_suffix('.json').read_text())\n")
            fake.chmod(0o755)
            legacy = json.loads(original)
            legacy["vocabulary"] = "oddlang-data-shapes-v1"
            profile.write_text(base.replace(str(HERE / 'run.sh'), str(fake)).replace(
                '["Cargo.toml", "Cargo.lock", "driver.rs", "shapes.rs", "shape_model.rs"]', '["legacy.json"]'))
            def replay(name):
                payload.write_text(json.dumps(dict(graph=legacy, crateName="observatory",
                                  compiler="8bab26f4f68e0e26f0bb7960be334d5b520ea452")))
                return subprocess.run([BINARY, "recover", "rust-shapes", manifest, "--profile",
                                       profile, "--out", root / name], capture_output=True, text=True)
            forged = replay("mislabeled")
            self.assertNotEqual(forged.returncode, 0)
            self.assertIn("array shape requires", forged.stderr)
            self.assertFalse((root / "mislabeled").exists())
            historical = next(d for d in legacy["declarations"] if d["name"] == "Frame")
            next(f for f in historical["fields"] if f["name"] == "array")["type"] = {
                "kind": "unresolved", "reason": "array-length"}
            old = replay("valid-old")
            self.assertEqual(old.returncode, 0, old.stderr)
            self.assertEqual(json.loads((root / "valid-old/shapes.lock").read_bytes())["vocabulary"],
                             "oddlang-data-shapes-v1")

            reformatted, _ = extract(SOURCE.replace("struct Marker;", "\n\nstruct Marker;"), "reformat")
            self.assertEqual(original, reformatted)

            mutation = SOURCE.replace("word: u16,\n    shared: Arc<T>,",
                                      "shared: Arc<T>,\n    word: u32,")
            mutation = mutation.replace("Zero,\n    One(T, u128),", "One(T, u128),\n    Zero,")
            mutation = mutation.replace("[u8; 3]", "[[u8; 0]; 2 + 2]")
            changed, mutated = extract(mutation, "mutation", feature=True)
            self.assertNotEqual(original, changed)
            new_fields = {field["name"]: field for field in mutated["Frame"]["fields"]}
            self.assertEqual(new_fields["word"]["type"]["bits"], 32)
            self.assertEqual(new_fields["word"]["id"], fields["word"]["id"])
            self.assertEqual(new_fields["word"]["position"], 1)
            nested = new_fields["array"]["type"]
            self.assertEqual(nested["extent"], {"kind": "fixed", "count": 4})
            self.assertEqual(nested["element"]["extent"], {"kind": "fixed", "count": 0})
            self.assertIn("extra", new_fields)
            self.assertEqual(mutated["Choice"]["variants"][0]["id"], variants[1]["id"])
            self.assertEqual(mutated["Choice"]["variants"][0]["position"], 0)


            secondary, network = extract(NETWORK, "secondary")
            self.assertEqual(set(network), {"Arc", "Route", "Status", "Window"})
            route = {field["name"]: field for field in network["Route"]["fields"]}
            self.assertEqual(route["alias"]["type"]["id"], network["Arc"]["id"])
            self.assertNotIn("ownership", route["alias"]["type"])
            self.assertEqual(route["cache"]["type"]["ownership"], "shared")
            self.assertEqual(route["next"]["type"]["ownership"], "exclusive")
            array = network["Window"]["fields"][1]["type"]
            self.assertEqual(array["element"]["id"], network["Window"]["parameters"][1]["id"])
            self.assertEqual(array["extent"], {"kind": "parameter", "id": network["Window"]["parameters"][2]["id"]})
            arguments = route["bytes"]["type"]["arguments"]
            self.assertEqual([arg["name"] for arg in arguments], ["'a", "T", "N"])
            self.assertEqual(arguments[0]["value"]["id"], network["Route"]["parameters"][0]["id"])
            self.assertEqual(arguments[2]["value"], {"kind": "unresolved", "reason": "constant-argument"})
            self.assertEqual(route["endpoint"]["type"]["lifetime"], {"kind": "static-lifetime"})
            self.assertEqual(route["offset"]["type"]["kind"], "target-integer")
            self.assertIn(
                {"owner": network["Status"]["variants"][0]["id"], "reason": "explicit-discriminant"},
                json.loads(secondary)["gaps"],
            )


if __name__ == "__main__":
    unittest.main()
