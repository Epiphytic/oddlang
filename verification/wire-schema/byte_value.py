"""Recover a single bounded byte field and exercise parser and validation separately."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

cli=Path(sys.argv[1]).resolve()
fixture=Path(__file__).resolve().parent/"fixture"
def write(p,obj):
    p.write_text(json.dumps(obj));return p
def run(*args,err=None,cwd=None):
    r=subprocess.run(args,cwd=cwd,capture_output=True,text=True)
    if err: assert r.returncode and err in r.stderr,r.stderr
    else: assert not r.returncode,r.stderr

with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp)
    source=root/"input.rs"
    source.write_text('''use serde::{Serialize,Deserialize};
#[derive(Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample { pub seq:u16, #[serde(with="base64_sample")] pub signature:Vec<u8> }
''')
    owner="urn:probe:sample"; field=owner+":field:signature"; error="urn:probe:bad"
    selection=dict(id="urn:probe:contract",types={"Sample":owner},
                   sourceSha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                   adapters={"base64_sample":{"kind":"bounded-base64","policy":{
                       "maxBytes":4,"encodedLimitMessage":"too long","decodedLimitMessage":"decoded length",
                       "syntaxMessage":"wrong encoding"}}})
    out=root/"observed"
    run(cli,"recover","wire-schema",source,write(root/"select.json",selection),out)
    g=json.loads((out/"graph.json").read_bytes());b=json.loads((out/"rust.json").read_bytes())
    assert g["types"][0]["fields"][1]["type"]["policy"]["maxBytes"]==4
    g["guards"]=[dict(id="urn:probe:length",owner=owner,require=dict(kind="count",field=field,minimum=1,maximum=4),error=error),
                 dict(id="urn:probe:byte",owner=owner,require=dict(kind="excludes-byte",field=field,byte=255),error=error)]
    b["nativeValidation"]={owner:{"method":"check","typePath":"crate::Failure","errors":{error:{"variant":"Invalid","argument":"signature"}}}}
    output=root/"generated"
    def project(o,err=None):
        run(cli,"build","wire-schema","rust",write(root/"g",g),write(root/"b",b),o,err=err)
    project(output)
    crate=root/"crate";(crate/"src").mkdir(parents=True)
    for name in ("Cargo.toml","Cargo.lock"): shutil.copyfile(fixture/name,crate/name)
    shutil.copyfile(output/"wire.rs",crate/"src/wire.rs")
    (crate/"src/main.rs").write_text(r'''#![deny(warnings)]
mod wire;
#[derive(Debug,PartialEq,Eq)]
pub enum Failure { Invalid(&'static str) }
fn main() {
    use wire::Sample;
    let bytes=r#"{"seq":2,"signature":"AP4="}"#;
    let s=serde_json::from_str::<Sample>(bytes).unwrap();
    assert_eq!(s.signature,[0,254]);
    assert_eq!(serde_json::to_string(&s).unwrap(),bytes);
    assert_eq!(s.check(),Ok(()));
    for path in ["","/w=="] {
        let bad=serde_json::from_value::<Sample>(serde_json::json!({"seq":0,"signature":path})).unwrap();
        assert_eq!(bad.check(),Err(Failure::Invalid("signature")));
    }
    for (s,want) in [("AA","wrong encoding"),("AB==","wrong encoding"),("AAAAAAA=","decoded length"),("?????????","too long")] {
        let err=serde_json::from_value::<Sample>(serde_json::json!({"seq":1,"signature":s})).err().unwrap().to_string();
        assert_eq!(err,want);
    }
}
''')
    run("cargo","+1.97.1","run","--quiet","--locked","--offline",cwd=crate)
    # Exercise the same graph through the complete source-free package builder.
    def pin(path):
        return {"path":path.name,"sha256":hashlib.sha256(path.read_bytes()).hexdigest()}
    shutil.copyfile(fixture/"Cargo.lock",root/"Cargo.lock")
    component=dict(kind="wire-schema",module="readings",visibility="public",reexports=["Sample"],
                   graph=pin(write(root/"graph.json",g)),overlay=pin(write(root/"rust.json",b)))
    failure="urn:probe:failure"
    variant="urn:probe:invalid"
    errors={"format":"oddlang-shared-declarations-v1","id":"urn:probe:outcome","errors":[
        {"id":failure,"variants":[{"id":variant,"payload":"text","message":[{"payload":True}]}]}]}
    names={"format":"oddlang-shared-rust-v1","names":{failure:"Failure",variant:"Invalid"},
           "variantOrder":{failure:[variant]}}
    shared=dict(kind="shared-declarations",module="outcomes",reexports=["Failure"],
                graph=pin(write(root/"errors.json",errors)),overlay=pin(write(root/"names.json",names)))
    manifest=dict(format="oddlang-native-package-v1",name="irrigation-schema-fixture",
                  version="0.0.0",edition="2021",dependencies={
                      "base64":{"version":"0.22.1"},"serde":{"version":"1.0.229","features":["derive"]},
                      "serde_json":{"version":"1.0.154"}},cargoLock=pin(root/"Cargo.lock"),components=[shared,component])
    package=root/"assembled"
    run(cli,"build","native-package","rust",write(root/"package.json",manifest),package)
    # A Cargo example is a distinct crate linked against the assembled library's public API.
    (package/"examples").mkdir()
    native=(crate/"src/main.rs").read_text().split("fn main()",1)[1]
    native=native.replace("use wire::Sample;", "use irrigation_schema_fixture::readings::Sample;")
    (package/"examples/probe.rs").write_text("use irrigation_schema_fixture::Failure;\nfn main()"+native)
    run("cargo","+1.97.1","run","--quiet","--locked","--offline","--example","probe",cwd=package)
    b["nativeValidation"]={owner:{"method":"check","typePath":"crate::Failure","errors":{}}}
    project(root/"bad",err="invalid native validation binding")
    g["guards"][1]["require"]["field"]=owner+":field:seq"
    project(root/"bad-type",err="predicate field type")
print("Bounded byte value recovery, canonical wire and typed rejection controls passed.")
