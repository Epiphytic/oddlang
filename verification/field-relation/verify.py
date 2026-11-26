"""Check field and fallible result relations with an unrelated runtime oracle."""
from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
iri = lambda name: "urn:invoice:" + name
word = dict(kind="text")
number = dict(kind="unsigned",bits=64)
named = lambda name:dict(kind="named",id=iri(name))
field = lambda obj,name:dict(kind="field",object=obj,field=iri(name))
arg = lambda name:dict(kind="argument",id=iri(name))
same = lambda left,right:dict(kind="equal",left=left,right=right)
call = lambda method,obj,*params:dict(method=iri(method),object=obj,arguments=[iri(p) for p in params])
argument = lambda label,value:dict(id=iri(label),value=value)

graph = {
    "format":"oddlang-field-relation-v1","id":iri("contract"),
    "types":{
        iri("Approval"):{"fields":{iri("owner"):word,iri("amount"):number}},
        iri("Quote"):{"fields":{iri("payee"):word}},
    },
    "calls":{
        iri("approval-valid"):{"receiver":iri("Approval"),"arguments":[]},
        iri("quote-total"):{"receiver":iri("Quote"),"arguments":[number],"output":number},
    },
    "relations":{
        iri("accepts"):{
            "receiver":iri("Approval"),"arguments":[argument("quote",named("Quote")),argument("units",number)],
            "failure":iri("mismatch"),
            "steps":[dict(kind="check",call=call("approval-valid","receiver")),
                     same(field("receiver","owner"),field(iri("quote"),"payee")),
                     same(field("receiver","amount"),dict(kind="result",call=call("quote-total",iri("quote"),"units")))],
        },
        iri("names-owner"):{
            "receiver":iri("Approval"),"arguments":[argument("candidate",word)],
            "steps":[same(field("receiver","owner"),arg("candidate"))],
        },
    },
}
binding = {
    "format":"oddlang-field-relation-rust-v1",
    "types":{iri(name):f"crate::{name}" for name in ("Approval","Quote")},
    "fields":{iri(name):name for name in ("owner","amount","payee")},
    "calls":{iri("approval-valid"):"valid",iri("quote-total"):"total"},
    "relations":{
        iri("accepts"):{"name":"accepts","arguments":{iri(name):name for name in ("quote","units")}},
        iri("names-owner"):{"name":"names_owner","arguments":{iri("candidate"):"candidate"}},
    },
    "errorType":"crate::Fault","errors":{iri("mismatch"):{"variant":"Reason","argument":"wrong quote"}},
}
native = '''
#![deny(warnings)]
mod relations;
use std::cell::Cell;
#[derive(Debug,PartialEq,Eq)]
pub enum Fault { Reason(&'static str) }
pub struct Approval { pub owner: String, pub amount: u64, pub active: bool }
pub struct Quote { pub payee: String, pub price: u64, pub calls: Cell<u64> }
impl Approval {
    pub fn valid(&self) -> Result<(),Fault> {
        if self.active { Ok(()) } else { Err(Fault::Reason("inactive")) }
    }
}
impl Quote {
    pub fn total(&self, units: u64) -> Result<u64,Fault> {
        self.calls.set(self.calls.get()+1);
        self.price.checked_mul(units).ok_or(Fault::Reason("overflow"))
    }
}
fn main() {
    let mut approval = Approval {owner:"north".into(), amount:20, active:false};
    let mut quote = Quote {payee:"south".into(),price:u64::MAX,calls:Cell::new(0)};
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("inactive")));
    approval.active = true;
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("wrong quote")));
    assert_eq!(quote.calls.get(),0);
    quote.payee = "north".into();
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("overflow")));
    quote.price=5;
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("wrong quote")));
    assert_eq!(approval.accepts(&quote,4), Ok(()));
    assert_eq!(quote.calls.get(),3);
    assert!(approval.names_owner("north"));
    assert!(!approval.names_owner("North"));
}
'''

def run(*args, error=None):
    result = subprocess.run(args,capture_output=True,text=True)
    if error:
        assert result.returncode and error in result.stderr,result
    else:
        assert not result.returncode,result


with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp)
    def project(g,b,dest,error=None):
        graph_path,binding_path=root/"graph.json",root/"rust.json"
        graph_path.write_text(json.dumps(g))
        binding_path.write_text(json.dumps(b))
        run(CLI,"build","field-relation","rust",graph_path,binding_path,dest,error=error)

    out=root/"out"
    project(graph,binding,out)
    (out/"main.rs").write_text(native)
    run("rustc","+1.97.1","--edition=2024",out/"main.rs","-o",root/"witness")
    run(root/"witness")
    for change,error in [
        (lambda g,b:g["relations"][iri("accepts")]["steps"][1]["right"].update(field=iri("amount")),
         "unknown field"),
        (lambda g,b:g["relations"][iri("accepts")]["steps"][1].update(right=arg("units")),
         "incompatible types"),
        (lambda g,b:g["relations"][iri("accepts")]["steps"][2]["right"]["call"].update(arguments=[]),
         "ill-typed fallible relation call"),
        (lambda g,b:g["relations"][iri("accepts")].update(failure=None),"ill-typed fallible relation call"),
        (lambda g,b:b["relations"][iri("accepts")].update(name="valid"),"duplicate native relation"),
    ]:
        g,b=deepcopy(graph),deepcopy(binding)
        change(g,b)
        project(g,b,root/"rejected",error)
        assert not (root/"rejected").exists()

print("Typed field equality and ordered checked calls pass independent runtime controls.")
