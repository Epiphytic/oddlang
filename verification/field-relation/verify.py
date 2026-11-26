"""Check field and fallible result relations with an unrelated runtime oracle."""
from copy import deepcopy
import hashlib
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

def run(*args, error=None, cwd=None):
    result = subprocess.run(args,capture_output=True,text=True,cwd=cwd)
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
    def unused(g,b):
        g["relations"][iri("names-owner")]["arguments"].append(argument("unused",number))
        b["relations"][iri("names-owner")]["arguments"][iri("unused")]="unused"

    def opaque(g,b):
        g["types"][iri("Denomination")]={"fields":{}}
        b["types"][iri("Denomination")]="crate::Denomination"
        g["calls"][iri("quote-total")]["arguments"]=[named("Denomination")]
        g["relations"][iri("accepts")]["arguments"][1]["value"]=named("Denomination")

    for change,error in [
        (unused, "unused argument"),
        (opaque, "requires Copy"),
        (lambda g,b:g["types"][iri("Approval")]["fields"][iri("owner")].update(extra=True),"unknown field"),
        (lambda g,b:g["relations"][iri("accepts")]["steps"][1].update(right=field("receiver","owner")),"reflexive comparison"),
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

    def pin(name, obj):
        path=root/name
        path.write_text(json.dumps(obj))
        return {"path":name,"sha256":hashlib.sha256(path.read_bytes()).hexdigest()}

    def part(kind, module, names, g, b):
        return dict(kind=kind,module=module,reexports=names,
                    graph=pin(module+".json",g),overlay=pin(module+"-rust.json",b))

    shared = part("shared-declarations","fault",["WireFault"],
        dict(format="oddlang-shared-declarations-v1",id=iri("fault"),
             errors=[dict(id=iri("WireFault"),variants=[dict(id=iri("Bad"),payload="text",message=[dict(payload=True)])])]),
        dict(format="oddlang-shared-rust-v1", names={iri("WireFault"):"WireFault",iri("Bad"):"Bad"},
             variantOrder={iri("WireFault"):[iri("Bad")]}))
    layouts = []
    amount = {}
    fault_types = ("value","truncated","trailing","decodeSize","encodeSize")
    for name in ("Invoice", "Remittance"):
        module=name.lower()
        amount[name]=iri(module+":amount")
        layout=[dict(id=amount[name],encoding=dict(kind="unsigned-be",bits=64,minimum=0,maximum=99))]
        errors={key:iri(module+":"+key) for key in fault_types}
        layouts.append(part("binary-record",module,[name],
            dict(format="oddlang-binary-record-v1",id=iri(name),layout=layout,maximum=8,errors=errors),
            dict(format="oddlang-binary-record-rust-v1",name=name,names={amount[name]:"amount"},
                 externals={},errorType="crate::WireFault",
                 errors={value:dict(variant="Bad",argument=key) for key,value in errors.items()})))
    remittance=iri("package-remittance")
    relation=part("field-relation","relation",[],
        dict(format="oddlang-field-relation-v1",id=iri("payment"),
             types={iri(name):{"fields":{field:number}} for name,field in amount.items()},calls={},
             relations={iri("agrees"):dict(receiver=iri("Invoice"),arguments=[dict(id=remittance,value=named("Remittance"))],
                 steps=[same(dict(kind="field",object="receiver",field=amount["Invoice"]),
                             dict(kind="field",object=remittance,field=amount["Remittance"]))])}),
        dict(format="oddlang-field-relation-rust-v1",types={iri(name):f"crate::{name}" for name in amount},
             fields={field:"amount" for field in amount.values()},calls={},errors={},
             relations={iri("agrees"):dict(name="agrees",arguments={remittance:"remittance"})}))

    (root/"src").mkdir()
    (root/"src/lib.rs").touch()
    (root/"Cargo.toml").write_text('[package]\nname="payment-fixture"\nversion="0.0.0"\nedition="2024"\n')
    run("cargo","+1.97.1","generate-lockfile","--offline",cwd=root)
    package=dict(format="oddlang-native-package-v1",name="payment-fixture",version="0.0.0",edition="2024",
                 cargoLock=dict(path="Cargo.lock",sha256=hashlib.sha256((root/"Cargo.lock").read_bytes()).hexdigest()),
                 components=[shared,*layouts,relation])
    manifest=root/"package.json"
    manifest.write_text(json.dumps(package))
    projected=root/"package"
    run(CLI,"build","native-package","rust",manifest,projected)
    (projected/"examples").mkdir()
    (projected/"examples/witness.rs").write_text(
        "use payment_fixture::{Invoice,Remittance};\nfn main() {assert!(Invoice{amount:25}.agrees(&Remittance{amount:25}));"
        "assert!(!Invoice{amount:25}.agrees(&Remittance{amount:26}));}\n"
    )
    run("cargo","+1.97.1","run","--locked","--offline","--example","witness",cwd=projected)
    relation["reexports"]= ["Invoice"]
    manifest.write_text(json.dumps(package))
    run(CLI,"build","native-package","rust",manifest,root/"bad-reexports",error="cannot reexport")

print("Typed field equality and ordered checked calls pass independent runtime controls.")
