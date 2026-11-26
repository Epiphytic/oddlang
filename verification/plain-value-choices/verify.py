#!/usr/bin/env python3
"""Check an unrelated ordinary fieldless choice and a source-order mutation."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

cli = Path(sys.argv[1]).resolve()
source = """#[derive(PartialEq, Copy, Eq, Clone)]
pub enum Compass { Inland, Coast, Ridge }
"""
caller = """
use compass::Compass;
fn need<T: Copy + Clone + Eq + PartialEq>(value: T) -> T { value.clone() }
fn ord(value: Compass) -> (&'static str, isize) {
    (match value {
        Compass::Inland => "inland",
        Compass::Coast => "coast",
        Compass::Ridge => "ridge",
    }, need(value) as isize)
}
fn main() {
    for value in [Compass::Inland, Compass::Coast, Compass::Ridge] {
        println!("{:?}", ord(value));
    }
}
"""


def run(*argv, **kwargs):
    return subprocess.run(argv, check=True, capture_output=True, text=True, **kwargs).stdout


with tempfile.TemporaryDirectory() as scratch:
    root = Path(scratch)
    src, selection = root / "compass.rs", root / "selection.json"

    def recover(text, output):
        src.write_text(text)
        selection.write_text(json.dumps(dict(
            format="oddlang-plain-choice-selection-v1", id="urn:compass:v1", name="Compass",
            sourceSha256=hashlib.sha256(text.encode()).hexdigest(),
        )))
        return subprocess.run([cli, "recover", "plain-choice", src, selection, output],
                              capture_output=True, text=True)

    consumer, library, binary = root / "main.rs", root / "libcompass.rlib", root / "probe"
    consumer.write_text(caller)
    outputs = []
    for label, text in (("baseline", source), ("mutant", source.replace("Inland, Coast", "Coast, Inland"))):
        extraction = root / label
        result = recover(text, extraction)
        assert result.returncode == 0, result.stderr
        projection = root / (label + "-rust")
        run(cli, "build", "checked-values", "rust", extraction / "graph.json", extraction / "rust.json", projection)
        observed = []
        for path in (src, projection / "values.rs"):
            run("rustc", "--edition=2021", "--crate-name=compass", "--crate-type=rlib", path, "-o", library)
            run("rustc", "--edition=2021", "--extern", f"compass={library}", consumer, "-o", binary)
            observed.append(run(binary))
        assert observed[0] == observed[1]
        outputs.append(observed[0])
    assert outputs[0].splitlines() == ['("inland", 0)', '("coast", 1)', '("ridge", 2)']
    assert outputs[1].splitlines() == ['("inland", 1)', '("coast", 0)', '("ridge", 2)']
    consumer.write_text('fn main() { println!("{:?}", compass::Compass::Inland); }')
    denied = subprocess.run(["rustc", "--edition=2021", "--extern", f"compass={library}",
                             consumer, "-o", binary], capture_output=True, text=True)
    assert denied.returncode and "Debug" in denied.stderr
    for index, text in enumerate((
        source.replace("Inland,", "Inland(u8),"),
        source.replace("Inland,", "Inland(),"),
        source.replace("Inland,", "Inland {},"),
        source.replace("Inland,", "Inland = 3,"),
        source.replace("#[derive", "#[repr(u8)]\n#[derive"),
        source.replace("#[derive", "#[non_exhaustive]\n#[derive"),
        source.replace("Eq, Clone", "Eq, Clone, Debug"),
        source + "\nimpl ::core::fmt::Debug for Compass {}",
        source + "\nmod other { impl ::core::fmt::Debug for super::Compass {} }",
        source + "\ntype Alias = Compass; impl ::core::fmt::Debug for Alias {}",
        source + "\nmod other { use super::Compass as Alias; impl ::core::fmt::Debug for Alias {} }",
        source + "\nimpl ::core::fmt::Debug for r#Compass {}",
        source + "\nimpl ::core::fmt::Debug for (Compass) {}",
        source + "\nimpl From<Compass> for u8 { fn from(_: Compass) -> Self { 7 } }",
        source + "\nimpl PartialEq<Compass> for u8 { fn eq(&self, _: &Compass) -> bool { false } }",
        source + r"""
trait Projection { type Choice; }
impl Projection for () { type Choice = Compass; }
impl ::core::fmt::Debug for <() as Projection>::Choice {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.write_str("compass")
    }
}
""",
        source + r"""
trait Projection { type Choice; }
impl Projection for () { type Choice = Compass; }
impl From<<() as Projection>::Choice> for u8 {
    fn from(_: <() as Projection>::Choice) -> Self { 7 }
}
""",
        source + r"""
macro_rules! display { () => {
    impl ::core::fmt::Debug for Compass {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            f.write_str("compass")
        }
    }
}; }
display!();
""",
        "use other::Replacement as Copy;\n" + source,
        source.replace("Inland,", "#[cfg(any())] Inland,"),
        source.replace("pub enum", "enum"),
    )):
        out = root / f"invalid-{index}"
        denied = recover(text, out)
        assert denied.returncode and not out.exists(), (text, denied.stderr)


    baseline = root / "baseline"
    graph = baseline / "graph.json"
    original_binding = json.loads((baseline / "rust.json").read_bytes())
    checked_id, = original_binding["types"]
    for index, (key, value) in enumerate((
        ("traits", ["copy", "hash"]),
        ("view", "as_label"),
        ("order", original_binding["types"][checked_id]["order"][::-1]),
    )):
        binding = json.loads((baseline / "rust.json").read_bytes())
        binding["types"][checked_id][key] = value
        overlay = root / "invalid-binding.json"
        overlay.write_text(json.dumps(binding))
        output = root / f"invalid-overlay-{index}"
        denied = subprocess.run([cli, "build", "checked-values", "rust", graph, overlay, output],
                                capture_output=True, text=True)
        assert denied.returncode and "plain choice native binding" in denied.stderr
        assert not output.exists()

    wrapped_graph = json.loads(graph.read_bytes())
    wrapped_graph["types"]["urn:compass:wrapper"] = dict(kind="record", fields={
        "urn:compass:wrapper:field": dict(kind="named", id=checked_id),
    })
    original_binding["types"]["urn:compass:wrapper"] = dict(
        name="Wrapper", order=["urn:compass:wrapper:field"], shape="named", members={
            "urn:compass:wrapper:field": dict(name="value", public=True),
        },
    )
    bad_graph = root / "invalid-graph.json"
    bad_graph.write_text(json.dumps(wrapped_graph))
    overlay.write_text(json.dumps(original_binding))
    denied = subprocess.run([cli, "build", "checked-values", "rust", bad_graph, overlay, root / "wrapper"],
                            capture_output=True, text=True)
    assert denied.returncode and "field lacks Debug" in denied.stderr
