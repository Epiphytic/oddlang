"""Check optional binary metadata and ordered constraints with an unrelated fixture."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
iri = lambda name: "urn:reading:" + name
err = lambda name: iri("error:" + name)

def slot(name, kind, **encoding):
    return dict(id=iri(name), encoding=dict(kind=kind, **encoding))

def guard(name, kind, **condition):
    return dict(id=iri("guard:" + name), error=err("metadata"), condition=dict(kind=kind, **condition))


graph = {
    "format":"oddlang-binary-record-v1", "id":iri("record"), "maximum":32,
    "layout":[slot("count", "unsigned-be", bits=16, minimum=0, maximum=10,
                   readError=err("counter"), valueError=err("counter")),
              slot("has_flags", "bool", error=err("bool")),
              slot("flags", "optional", payload=dict(kind="unsigned-be",bits=8), error=err("option")),
              slot("position", "optional", payload=dict(kind="record",id=iri("Position")), error=err("option")),
              slot("seed", "fixed-bytes", size=3),
              slot("route", "enum-u8", external=iri("Route"), tags={iri("Local"):7}, error=err("route")),
              slot("cluster", "bytes", minimum=1, maximum=4, excludes=[0], overflow=err("large"),
                   valueError=err("cluster"))],
    "errors":{"value":err("counter"),"decodeSize":err("large"),"encodeSize":err("large"),
              "truncated":err("short"),"trailing":err("extra")},
    "records":{iri("Position"):{"fields":[dict(id=iri("degrees"),bits=64,signed=True),dict(id=iri("fraction"),bits=16)]}},
    "guards":[guard("presence","presence-equals",flag=iri("has_flags"),optional=iri("flags")),
              guard("mask","optional-mask",optional=iri("flags"),allowed=7,required=2),
              guard("fraction","optional-member-range",optional=iri("position"),member=iri("fraction"),
                    minimum=0,maximum=999)],
    "hashViews":{iri("index"):{"algorithm":"sha256","domain":[5,0],"fields":[iri("seed"),iri("route")],"prefix":"r-"}},
}
binding = {"format":"oddlang-binary-record-rust-v1","name":"Reading",
           "names":{iri(name):name for name in
                    ("count","has_flags","flags","position","seed","Position","degrees","fraction","route","cluster")},
           "externals":{iri("Route"):{"path":"crate::Route","variants":{iri("Local"):"Local"}}}, "errorType":"crate::Fault",
           "errors":{err(name):dict(variant="Code",argument=name) for name in
                     ("bool","option","large","short","extra","metadata","counter","route","cluster")},
           "hashViews":{iri("index"):dict(method="index",library="sha2")}}

def run(*args, cwd=None, error=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if error:
        assert result.returncode and error in result.stderr, result
    else:
        assert not result.returncode, result


with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    def project(out, error=None):
        graph_file, binding_file = root/"graph.json", root/"rust.json"
        graph_file.write_text(json.dumps(graph))
        binding_file.write_text(json.dumps(binding))
        run(CLI,"build","binary-record","rust",graph_file,binding_file,out,error=error)

    out = root/"src"
    project(out)
    (root/"Cargo.toml").write_text('''[package]
name = "metadata-fixture"
version = "0.0.0"
edition = "2024"

[dependencies]
sha2 = "=0.10.9"
''')
    digest = hashlib.sha256(bytes([5,0,16,32,48,7])).hexdigest()
    (out/"main.rs").write_text('''#![deny(warnings)]
mod record;
use record::{Reading, Position};
#[derive(Debug, PartialEq, Eq)]
pub enum Fault { Code(&'static str) }
#[derive(Clone,PartialEq,Eq)]
pub enum Route { Local }
fn main() {
    let value = Reading { count: 10, has_flags: true, flags: Some(3),
        position: Some(Position { degrees: i64::MIN, fraction: 999 }), seed: [16,32,48],
        route: Route::Local, cluster: vec![42] };
    const BYTES: [u8; 25] = [0,10,1,1,3,1,128,0,0,0,0,0,0,0,3,231,16,32,48,7,0,0,0,1,42];
    assert_eq!(value.encode().unwrap(), BYTES);
    assert!(Reading::decode(&BYTES).unwrap() == value);
    assert_eq!(value.index(), "r-DIGEST");
    let mut invalid = BYTES.to_vec();
    invalid[15] = 232;
    assert_eq!(Reading::decode(&invalid).err(), Some(Fault::Code("metadata")));
    invalid.push(7);
    assert_eq!(Reading::decode(&invalid).err(), Some(Fault::Code("extra")));
    assert_eq!(Reading::decode(&[0,11,9]).err(), Some(Fault::Code("counter")));
    assert_eq!(Reading::decode(&[0,10,9]).err(), Some(Fault::Code("bool")));
    assert_eq!(Reading::decode(&[0,10,1,9]).err(), Some(Fault::Code("option")));
    assert_eq!(Reading::decode(&[0,10,1,1]).err(), Some(Fault::Code("short")));
    let mut absent = value.clone();
    absent.flags = None;
    assert_eq!(absent.encode().err(), Some(Fault::Code("metadata")));
    absent.has_flags = false;
    absent.position = None;
    assert_eq!(absent.encode().unwrap(), [0,10,0,0,0,16,32,48,7,0,0,0,1,42]);
    assert!(Reading::decode(&absent.encode().unwrap()).unwrap() == absent);
    let mut bad = value.clone();
    bad.flags = Some(8);
    assert_eq!(bad.validate(), Err(Fault::Code("metadata")));
    assert_eq!(bad.index(), value.index());
    bad.cluster = vec![0];
    assert_eq!(bad.validate(), Err(Fault::Code("cluster")));
}
'''.replace("DIGEST",digest))
    run("cargo","+1.97.1","generate-lockfile","--offline",cwd=root)
    run("cargo","+1.97.1","run","--locked","--offline",cwd=root)
    graph["guards"][1]["condition"]["required"] = 8
    project(root/"bad-mask",error="invalid binary record guard")
    graph["guards"][1]["condition"]["required"] = 2
    binding["hashViews"][iri("index")]["method"] = "clone"
    project(root/"bad-method",error="invalid binary hash method")

print("Optional binary metadata, checked masks and unvalidated hash views match independent outcomes.")
