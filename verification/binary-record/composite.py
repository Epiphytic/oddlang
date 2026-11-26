"""Witness nested binary values, checked arithmetic and footer precedence."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI=Path(sys.argv[1]).resolve()
iri=lambda value:"urn:survey:"+value
fault=lambda value:iri("error:"+value)
cell=lambda name,kind,**kw:dict(id=iri(name),encoding=dict(kind=kind,**kw))
op=lambda kind,**kw:dict(kind=kind,**kw)

def graph(name,layout,**kw):
    return dict(format="oddlang-binary-record-v1",id=iri(name),maximum=128,layout=layout,
                errors={key:fault(value) for key,value in [("value","value"),("truncated","short"),
                    ("trailing","trailing"),("decodeSize","limit"),("encodeSize","limit")]},**kw)

def binding(name,fields,errs,**kw):
    return dict(format="oddlang-binary-record-rust-v1",name=name,names={iri(key):value for key,value in fields.items()},
        externals={},errorType="crate::Failure",
        errors={fault(error):dict(variant="Bad",argument=error) for error in errs},**kw)

basic=("value","short","trailing","limit")
inner=graph("header-type",[cell("quantity","unsigned-be",bits=32,minimum=1,maximum=9)])
outer=graph("sample",[
    cell("magic","literal",bytes=[77,3],requireFull=True,error=fault("magic")),
    cell("header","nested",record=iri("header-type"),maximum=8,overflow=fault("limit")),
    cell("first","signed-be",bits=64),cell("last","signed-be",bits=64),
    cell("position","inline",id=iri("point")),
    cell("cached","optional",payload=op("record",id=iri("point")),error=fault("tag")),
    cell("count","unsigned-be",bits=32,minimum=0,maximum=9),
], externalRecords={iri("header-type"):{"fields":{iri("quantity"):op("unsigned",bits=32)}}},
 records={iri("point"):{"fields":[dict(id=iri("north"),bits=16,signed=True)]}},
 constraints=[dict(id=iri("duration"),error=fault("period"),predicate=op("elapsed",
                      end=op("field",id=iri("last")),start=op("field",id=iri("first")),minimum=1)),
              dict(id=iri("stable"),error=fault("point"),predicate=op("optional-equals",
                      field=iri("cached"),other=iri("position"))),
              dict(id=iri("bounds"),error=fault("recount"),predicate=op("at-most",left=op("field",id=iri("count")),
                      right=op("member",field=iri("header"),id=iri("quantity"))))],
 integrity=dict(algorithm="sha256",domain=[74,0],short=fault("limit"),mismatch=fault("checksum")))
native=binding("Sample", {name:name for name in ("header","first","last","north","position","cached","count")}|
               {"point":"Point"}, (*basic,"magic","period","point","recount","checksum","tag"),
               externalRecords={iri("header-type"):{"path":"crate::Header","fields":{iri("quantity"):"quantity"}}},
               integrity="sha2")

def run(*args,cwd=None,fail=None):
    result=subprocess.run(args,cwd=cwd,capture_output=True,text=True)
    if fail:
        assert result.returncode and fail in result.stderr,result
    else:
        assert not result.returncode,result

with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp)
    def project(g,b,out,fail=None):
        gp,bp=root/"graph.json",root/"rust.json"
        gp.write_text(json.dumps(g));bp.write_text(json.dumps(b))
        run(CLI,"build","binary-record","rust",gp,bp,out,fail=fail)

    project(inner,binding("Header",{"quantity":"quantity"},basic),root/"inner")
    project(outer,native,root/"src")
    (root/"src/inner.rs").write_bytes((root/"inner/record.rs").read_bytes())
    (root/"Cargo.toml").write_text('[package]\nname="survey"\nversion="0.0.0"\nedition="2024"\n[dependencies]\nsha2="=0.10.9"\n')
    payload=bytes([77,3,0,0,0,4,0,0,0,1])+(-1).to_bytes(8,"big",signed=True)+bytes(8)+bytes([255,254,0,0,0,0,1])
    def seal(data):return list(data+hashlib.sha256(bytes([74,0])+data).digest())
    (root/"src/main.rs").write_text(f'''#![deny(warnings)]
mod inner;
mod record;
pub use inner::Header;
use record::{{Point,Sample}};
#[derive(Debug,PartialEq,Eq)]
pub enum Failure {{ Bad(&'static str) }}
fn main() {{
    let mut s=Sample {{header:Header{{quantity:1}},first:-1,last:0,position:Point{{north:-2}},cached:None,count:1}};
    let bytes={seal(payload)!r};
    assert_eq!(s.encode().unwrap(),bytes);
    assert!(Sample::decode(&bytes).unwrap()==s);
    assert_eq!(Sample::decode(&[0;31]).err(),Some(Failure::Bad("limit")));
    assert_eq!(Sample::decode(&[0;32]).err(),Some(Failure::Bad("checksum")));
    assert_eq!(Sample::decode(&{seal(payload[:1])!r}).err(),Some(Failure::Bad("short")));
    assert_eq!(Sample::decode(&{seal(b"bad")!r}).err(),Some(Failure::Bad("magic")));
    assert_eq!(Sample::decode(&{seal(payload[:6]+bytes(4))!r}).err(),Some(Failure::Bad("value")));
    s.last=i64::MAX;
    assert_eq!(s.validate(),Err(Failure::Bad("period")));
    s.last=1;
    s.cached=Some(Point{{north:2}});
    assert_eq!(s.validate(),Err(Failure::Bad("point")));
    s.cached=None;
    s.count=2;
    assert_eq!(s.validate(),Err(Failure::Bad("recount")));
}}
''')
    run("cargo","+1.97.1","generate-lockfile","--offline",cwd=root)
    run("cargo","+1.97.1","run","--locked","--offline",cwd=root)
    outer["constraints"][2]["predicate"]["right"]["id"]=iri("ghost")
    project(outer,native,root/"rejected",fail="invalid composite record constraint")

print("Composite binary values, checked nested validation and checksum precedence pass.")
