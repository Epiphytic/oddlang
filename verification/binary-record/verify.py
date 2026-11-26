"""Check generated telemetry record bytes independently of the projector."""
from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
base = "urn:telemetry:"
id = lambda name: base + name
errors = {id(name): {"variant": "Bad", "argument": name}
          for name in ("record", "limit", "header", "short", "extra", "encoding", "tag", "trailer")}
fields = [
    dict(kind="literal", id=id("header"), bytes=[84, 0], error=id("header")),
    dict(kind="unsigned-be", id=id("counter"), bits=32, minimum=2, maximum=10),
    dict(kind="fixed-bytes", id=id("fingerprint"), size=2),
    dict(kind="bytes", id=id("payload"), minimum=1, maximum=6, overflow=id("limit")),
    dict(kind="enum-u8", id=id("state"), external=id("Mode"), tags={id("New"): 2, id("Closed"): 4},
         error=id("tag")),
    dict(kind="ascii-atom", id=id("label"), minimum=1, maximum=3, punctuation=[95],
         overflow=id("limit"), error=id("encoding")),
    dict(kind="length-prefixed-literal", id=id("trailer"), bytes=[33], maximum=2,
         overflow=id("limit"), error=id("trailer")),
]
fields = [dict(id=slot["id"], encoding={k:v for k,v in slot.items() if k != "id"}) for slot in fields]
graph = dict(format="oddlang-binary-record-v1", id=id("record"), layout=fields, maximum=27,
             errors=dict(value=id("record"), decodeSize=id("header"), encodeSize=id("limit"),
                         truncated=id("short"), trailing=id("extra")))
binding = dict(format="oddlang-binary-record-rust-v1", name="Sample",
               names={id(n): n for n in ("counter", "fingerprint", "payload", "label", "state")},
               externals={id("Mode"): {"path":"crate::Mode", "variants":{id("New"):"New", id("Closed"):"Closed"}}},
               errorType="crate::Error", errors=errors)
source = r'''
#![deny(warnings)]
mod record;
use record::Sample;
#[derive(Clone, PartialEq, Eq)]
pub enum Mode { New, Closed }
#[derive(Debug, PartialEq, Eq)]
pub enum Error { Bad(&'static str) }
fn main() {
    let mut value = Sample { counter: 5, fingerprint: [0xf0, 0xff],
        payload: vec![0, 0xff], state: Mode::Closed, label: "z_".into() };
    const EXPECTED: &[u8] = &[84, 0, 0,0,0,5, 240,255, 0,0,0,2,0,255, 4, 0,0,0,2,122,95, 0,0,0,1,33];
    // The 26-byte record includes a total bound tighter than the sum of all maxima.
    assert_eq!(value.encode().unwrap(), EXPECTED);
    assert!(Sample::decode(EXPECTED).unwrap() == value);
    for prefix in 0..EXPECTED.len() {
        let expected = if prefix < 2 { "header" } else { "short" };
        assert_eq!(Sample::decode(&EXPECTED[..prefix]).err(),Some(Error::Bad(expected)));
    }
    let mut bad = EXPECTED.to_vec();
    bad[14] = 9;
    bad[5] = 1;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("tag")));
    bad[14] = 2;
    bad.push(0);
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("extra")));
    value.counter = 0;
    assert_eq!(value.encode().err(), Some(Error::Bad("record")));
    value.counter = 5;
    value.payload = vec![0; 6];
    assert_eq!(value.validate(), Ok(()));
    assert_eq!(value.encode().err(), Some(Error::Bad("limit")));
}
'''


def run(*args, cwd=None, error=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if error:
        assert result.returncode and error in result.stderr, result
    else:
        assert result.returncode == 0, result


with tempfile.TemporaryDirectory() as path:
    root = Path(path)

    def project(g, b, output, error=None):
        gp, bp = root/"graph.json", root/"rust.json"
        gp.write_text(json.dumps(g))
        bp.write_text(json.dumps(b))
        run(CLI, "build", "binary-record", "rust", gp, bp, output, error=error)

    out = root/"out"
    project(graph, binding, out)
    (out/"main.rs").write_text(source)
    run("rustc", "+1.97.1", "--edition=2024", out/"main.rs", "-o", root/"witness")
    run(root/"witness")
    doc = json.loads((out/"obligations.json").read_bytes())
    assert {row["declaration"] for row in doc["obligations"]} == {graph["id"], *[slot["id"] for slot in fields]}
    for change, diagnostic in [
        (lambda g,b: g["layout"][1]["encoding"].update(bits=12), "invalid unsigned"),
        (lambda g,b: g["layout"][4]["encoding"]["tags"].update({id("New"):4}), "invalid enum"),
        (lambda g,b: g["layout"][5]["encoding"].update(punctuation=[255]), "invalid ASCII"),
        (lambda g,b: b["names"].update({id("label"):"counter"}), "field name"),
    ]:
        g,b = deepcopy(graph),deepcopy(binding)
        change(g,b)
        project(g,b,root/"rejected",error=diagnostic)
        assert not (root/"rejected").exists()

print("Portable tagged record agrees with independent bytes, bounds and named errors.")
