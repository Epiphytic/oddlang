"""Exercise checked tagged binary messages and poisoned incremental input independently."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI=Path(sys.argv[1]).resolve()
iri=lambda key:"urn:meter:"+key
err=lambda key:iri("error:"+key)
slot=lambda key,kind,**kw:dict(id=iri(key),encoding=dict(kind=kind,**kw))
guard=lambda name,error,kind,**kw:dict(id=iri(name),error=err(error),require=dict(kind=kind,**kw))
count=guard("count","data","count",field=iri("data"))
end=lambda name:guard(name,"range","end-at-most",offset=iri("index"),bytes=iri("data"),maximum=12)
cases={
    iri("panel"):dict(tag=22,form="tuple",slots=[slot("device","fragment",id=iri("header"))]),
    iri("samples"):dict(tag=23,form="named",slots=[slot("index","unsigned",bits=64,maximum=12,error=err("range")),
                      slot("data","rest",minimum=1,maximum=8,error=err("data"))],
                      encodeGuards=[count,end("encode-end")],decodeGuards=[end("decode-end")]),
    iri("existing-case"):dict(tag=25,form="tuple",slots=[slot("existing","fragment",id=iri("existing-type"))]),
    iri("seal"):dict(tag=24,form="tuple",slots=[slot("seal-id","fixed",size=2),
                      slot("stamp","literal",value=91,error=err("stamp"))]),
}
errors={k:err(k) for k in ("header","length","limit","tag","truncated","trailing","decoderFailed")}
graph=dict(format="oddlang-binary-frame-v1",id=iri("messages"),maximum=16,errors=errors,
           types={iri("header"):dict(kind="fragment"),iri("existing-type"):dict(kind="fragment")},cases=cases)
binding=dict(format="oddlang-binary-frame-rust-v1",name="Message",decoder="Messages",errorType="crate::Fault",
             cases={iri("panel"):"Panel",iri("samples"):"Samples",iri("seal"):"Seal",iri("existing-case"):"Existing"},
             fields={iri("device"):"device",iri("index"):"index",iri("data"):"data",iri("seal-id"):"seal_id",iri("existing"):"existing"},
             types={iri("header"):dict(path="crate::Header"),iri("existing-type"):dict(path="crate::Existing")},
             errors={err(name):dict(variant="Bad",argument=name) for name in
                     (*errors,"data","range","stamp")})
device=dict(format="oddlang-binary-record-v1",id=iri("header"),fragment=True,maximum=16,
            layout=[slot("unit","unsigned-be",bits=16,minimum=1,maximum=13)],
            errors=dict(value=err("unit"),encodeSize=err("limit"),decodeSize=err("limit"),
                        trailing=err("trailing"),truncated=err("truncated")))
overlay=dict(format="oddlang-binary-record-rust-v1",name="Header",names={iri("unit"):"unit"},
             externals={},errorType="crate::Fault",errors={err(name):dict(variant="Bad",argument=name)
                      for name in ("unit","limit","trailing","truncated")})

existing=dict(device,id=iri("existing-type"),maximum=7,layout=[
    slot("quantity","unsigned-be",bits=16,minimum=1,maximum=13),
    slot("sample","bytes",minimum=1,maximum=8,overflow=err("limit")),
])
existing_native=dict(overlay,name="Existing",existingType="crate::Existing",
                     names={iri("quantity"):"quantity",iri("sample"):"sample"})

def run(*args,error=None):
    result=subprocess.run(args,capture_output=True,text=True)
    if error: assert result.returncode and error in result.stderr,result
    else: assert not result.returncode,result

with tempfile.TemporaryDirectory() as directory:
    root=Path(directory)
    def project(profile,g,b,out,error=None):
        gp,bp=root/"graph.json",root/"rust.json"
        gp.write_text(json.dumps(g));bp.write_text(json.dumps(b))
        run(CLI,"build",profile,"rust",gp,bp,out,error=error)
    project("binary-frame",graph,binding,root/"src")
    project("binary-record",device,overlay,root/"inner")
    (root/"src/record.rs").write_bytes((root/"inner/record.rs").read_bytes())
    project("binary-record",existing,existing_native,root/"bound")
    (root/"src/bound.rs").write_bytes((root/"bound/record.rs").read_bytes())
    (root/"src/main.rs").write_text(r'''#![deny(warnings)]
mod frame;
mod record;
mod bound;
pub use record::Header;
#[derive(Clone, Eq, PartialEq)]
pub struct Existing { quantity:u16, sample:Vec<u8> }
impl Existing { pub fn validate(&self) -> Result<(),Fault> { Ok(()) } }
use frame::{Message,Messages};
#[derive(Debug,PartialEq,Eq)]
pub enum Fault { Bad(&'static str) }
fn main() {
    let seed=Message::Panel(Header{unit:1});
    let bytes=seed.encode().unwrap();
    assert_eq!(bytes,[22,0,0,0,2,0,1]);
    for split in 0..bytes.len() {
        let mut decoder=Messages::default();
        let (used,frame)=decoder.feed(&bytes[..split]).unwrap();
        assert_eq!(used,split);
        assert!(frame.is_none());
        let (used,frame)=decoder.feed(&[&bytes[split..], &bytes].concat()).unwrap();
        assert_eq!(used,bytes.len()-split);
        assert!(frame==Some(seed.clone()));
        assert_eq!(decoder.finish(),Ok(()));
    }
    assert_eq!(Message::decode(&[22,0,0,0,3,0,0,1]).err(),Some(Fault::Bad("unit")));
    assert_eq!(Message::decode(&[25,0,0,0,7,0,0,0,0,0,1,7]).err(),Some(Fault::Bad("unit")));
    assert_eq!(Message::decode(&[25,0,0,0,8,0,1,0,0,0,2,7,8]).err(),Some(Fault::Bad("limit")));
    let valid=Message::Existing(Existing{quantity:1,sample:vec![7]});
    assert!(Message::decode(&valid.encode().unwrap()).unwrap()==valid);
    assert_eq!(Message::decode(&[24,0,0,0,2,0,1]).err(),Some(Fault::Bad("truncated")));
    assert_eq!(Message::decode(&[24,0,0,0,3,0,1,1]).err(),Some(Fault::Bad("stamp")));
    assert_eq!(Message::Samples{index:13,data:vec![]}.encode().err(),Some(Fault::Bad("data")));
    assert_eq!(Message::decode(&[23,0,0,0,8,0,0,0,0,0,0,0,13]).err(),Some(Fault::Bad("range")));
    assert_eq!(Message::decode(&[0,0,0,0,17]).err(),Some(Fault::Bad("limit")));
    let mut decoder=Messages::default();
    assert_eq!(decoder.feed(&[22,0,0,0,17]).err(),Some(Fault::Bad("limit")));
    assert_eq!(decoder.feed(&[]).err(),Some(Fault::Bad("decoderFailed")));
    assert_eq!(decoder.finish(),Err(Fault::Bad("truncated")));
}
''')
    run("rustc","+1.97.1","--edition=2024",root/"src/main.rs","-o",root/"test")
    run(root/"test")
    graph["cases"][iri("samples")]["encodeGuards"].clear()
    project("binary-frame",graph,binding,root/"rejected",error="missing frame encode bound")
print("Tagged binary cases, nested checks and incremental poisoning pass.")
