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
fault = lambda name: id("error:" + name)
errors = {fault(name): {"variant": "Bad", "argument": name} for name in (
    "record", "decode-size", "encode-size", "header", "short", "extra", "encoding", "tag",
    "trailer", "payload-overflow", "atom-overflow", "trailer-overflow")}
fields = [
    dict(kind="literal", id=id("header"), bytes=[84, 0], error=fault("header")),
    dict(kind="unsigned-be", id=id("counter"), bits=32, minimum=2, maximum=10),
    dict(kind="fixed-bytes", id=id("fingerprint"), size=2),
    dict(kind="bytes", id=id("payload"), minimum=1, maximum=6, overflow=fault("payload-overflow")),
    dict(kind="enum-u8", id=id("state"), external=id("Mode"), tags={id("New"): 2, id("Closed"): 4},
         error=fault("tag")),
    dict(kind="ascii-atom", id=id("label"), minimum=1, maximum=3, punctuation=[95],
         overflow=fault("atom-overflow"), error=fault("encoding")),
    dict(kind="length-prefixed-literal", id=id("trailer"), bytes=[33], maximum=2,
         overflow=fault("trailer-overflow"), error=fault("trailer")),
]
fields = [dict(id=slot["id"], encoding={k:v for k,v in slot.items() if k != "id"}) for slot in fields]
graph = dict(format="oddlang-binary-record-v1", id=id("record"), layout=fields, maximum=27,
             errors=dict(value=fault("record"), decodeSize=fault("decode-size"), encodeSize=fault("encode-size"),
                         truncated=fault("short"), trailing=fault("extra")))
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
    bad[5] = 1;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("record")));
    bad[14] = 9;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("tag")));
    bad[14] = 2;
    bad.push(0);
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("extra")));
    bad.push(0);
    bad[0] = 0;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("decode-size")));
    for (offset, name) in [(8, "payload-overflow"), (15, "atom-overflow"), (21, "trailer-overflow")] {
        let mut corrupt = EXPECTED[..offset + 4].to_vec();
        corrupt[offset + 3] = 7;
        assert_eq!(Sample::decode(&corrupt).err(), Some(Error::Bad(name)));
    }
    for (offset, name) in [(19, "encoding"), (25, "trailer")] {
        let mut corrupt = EXPECTED.to_vec();
        corrupt[offset] = b'?';
        assert_eq!(Sample::decode(&corrupt).err(), Some(Error::Bad(name)));
    }
    value.counter = 0;
    assert_eq!(value.encode().err(), Some(Error::Bad("record")));
    value.counter = 5;
    value.payload = vec![0; 6];
    assert_eq!(value.validate(), Ok(()));
    assert_eq!(value.encode().err(), Some(Error::Bad("encode-size")));
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

    def write(name, obj):
        path = root/name
        path.write_text(json.dumps(obj))
        return path

    def project(g, b, output, error=None):
        gp, bp = write("graph.json", g), write("rust.json", b)
        run(CLI, "build", "binary-record", "rust", gp, bp, output, error=error)

    out = root/"out"
    project(graph, binding, out)
    (out/"main.rs").write_text(source)
    run("rustc", "+1.97.1", "--edition=2024", out/"main.rs", "-o", root/"witness")
    run(root/"witness")
    doc = json.loads((out/"obligations.json").read_bytes())
    assert {row["declaration"] for row in doc["obligations"]} == {graph["id"], *[slot["id"] for slot in fields]}
    def collide_error(g, b):
        g["errors"]["value"] = g["layout"][1]["id"]
        b["errors"][g["errors"]["value"]] = b["errors"].pop(fault("record"))

    for change, diagnostic in [
        (collide_error, "binary error aliases"),
        (lambda g,b: b.update(errorType="Error"), "invalid binary record"),
        (lambda g,b: b["externals"][id("Mode")].update(path="Mode"), "invalid enum binding"),
        (lambda g,b: g["layout"][1]["encoding"].update(bits=12), "invalid unsigned"),
        (lambda g,b: g["layout"][4]["encoding"]["tags"].update({id("New"):4}), "invalid enum"),
        (lambda g,b: g["layout"][5]["encoding"].update(punctuation=[255]), "invalid ASCII"),
        (lambda g,b: b["names"].update({id("label"):"counter"}), "field name"),
    ]:
        g,b = deepcopy(graph),deepcopy(binding)
        change(g,b)
        project(g,b,root/"rejected",error=diagnostic)
        assert not (root/"rejected").exists()

    import hashlib

    definitions = dict(
        format="oddlang-shared-declarations-v1", id=id("native-shared"),
        errors=[dict(id=id("Error"), variants=[dict(id=id("Bad"), payload="text", message=[dict(payload=True)])])],
        stringEnums=[dict(id=id("Mode"), variants=[dict(id=id(name), wire=name.lower())
                                                   for name in ("New", "Closed")])])
    names = dict(format="oddlang-shared-rust-v1",
                 names={id(name): name for name in ("Error", "Bad", "Mode", "New", "Closed")},
                 variantOrder={id("Error"): [id("Bad")], id("Mode"): [id("New"), id("Closed")]})

    def pin(path):
        return dict(path=path.name, sha256=hashlib.sha256(path.read_bytes()).hexdigest())

    (root/"src").mkdir()
    (root/"src/lib.rs").touch()
    (root/"Cargo.toml").write_text('''[package]
name = "telemetry-fixture"
version = "0.0.0"
edition = "2024"
[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
''')
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)
    manifest = dict(format="oddlang-native-package-v1", name="telemetry-fixture", version="0.0.0", edition="2024",
                    dependencies={"serde":dict(version="1.0.229", features=["derive"])},
                    cargoLock=pin(root/"Cargo.lock"), components=[
                        dict(kind="shared-declarations", module="shared", reexports=["Error","Mode"],
                             graph=pin(write("definitions.json", definitions)), overlay=pin(write("names.json", names))),
                        dict(kind="binary-record", module="frame", reexports=["Sample"],
                             graph=pin(write("graph.json", graph)), overlay=pin(write("rust.json", binding))),
                    ])
    package = root/"package"
    run(CLI, "build", "native-package", "rust", write("manifest.json", manifest), package)
    examples = package/"examples"
    examples.mkdir()
    (examples/"witness.rs").write_text(
        "use telemetry_fixture::{Error, Mode, Sample};\nfn main()" + source.split("fn main()",1)[1]
    )
    run("cargo", "+1.97.1", "run", "--locked", "--offline", "--example", "witness", cwd=package)

print("Portable tagged record agrees with independent bytes, bounds and named errors.")
