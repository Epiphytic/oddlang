"""Build an unrelated checked-value domain and test it through its native API."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

DOMAIN = "urn:example:detector:"
n = lambda name: DOMAIN+name
FAULT = n("invalid")


def fixture(bound=6):
    targets = {
        n("tone"): dict(kind="enum", values={n("quiet"):"quiet",n("loud"):"loud"}, failure=FAULT),
        n("octave"): dict(kind="enum", values={n("low"):1,n("high"):7}),
        n("caption"): dict(kind="record",fields={n("caption:text"):{"kind":"text"}}, constructor=FAULT,
                         guards=[dict(kind="utf8-byte-length",field=n("caption:text"),minimum=1,maximum=bound),
                                 dict(kind="exclude-unicode",field=n("caption:text"),generalCategoryCc=True,
                                      ranges=[{"minimum":0x2066,"maximum":0x2069}])]),
        n("station"): dict(kind="record",fields={
            n("station:code"):{"kind":"text"},n("station:number"):{"kind":"u8"},
            n("station:tone"):{"kind":"named","id":n("tone")}},
            constructor=FAULT,guards=[
                dict(kind="one-of",field=n("station:number"),values=[2,4]),
                dict(kind="utf8-byte-length",field=n("station:code"),minimum=2,maximum=2),
                dict(kind="ascii-bytes",field=n("station:code"),ranges=[{"minimum":65,"maximum":90}])]),
        n("metadata"):dict(kind="record", fields={n("metadata:station"):{"kind":"named","id":n("station")},
                                                  n("metadata:caption"):{"kind":"named","id":n("caption")}}),
    }
    bindings = {
        n("tone"):dict(name="Tone",order=[n("quiet"),n("loud")],traits=["copy","hash"],view="speaker",parse="identify"),
        n("octave"):dict(name="Octave",order=[n("low"),n("high")],traits=["copy"],view="index"),
        n("caption"):dict(name="Caption",order=[n("caption:text")],shape="tuple",constructor="make",
                          members={n("caption:text"):dict(name="value",getter="text")}),
        n("station"):dict(name="Station",order=[n("station:number"),n("station:tone"),n("station:code")],
                          shape="named",constructor="make",traits=["hash"],members={
                            n("station:number"):dict(name="number",getter="number"),
                            n("station:tone"):dict(name="tone",getter="tone"),
                            n("station:code"):dict(name="code",getter="code")}),
        n("metadata"):dict(name="Metadata",order=[n("metadata:station"),n("metadata:caption")],shape="named",
                          members={n("metadata:station"):dict(name="station",public=True),
                                   n("metadata:caption"):dict(name="caption",public=True)}),
    }
    names={n("quiet"):"Quiet",n("loud"):"Loud",n("low"):"Low",n("high"):"High"}
    return (dict(format="oddlang-checked-values-v1",id=n("domain"),types=targets),
            dict(format="oddlang-checked-values-rust-v1",types=bindings,variants=names,
                 errors={FAULT:dict(errorType="crate::Fault",variant="Invalid")}))


def write(path, data):
    path.write_text(json.dumps(data, indent=2)+"\n")
    return path


CALLER = r"""
use detector::{Caption, Fault, Metadata, Octave, Station, Tone};
fn main() {
    assert_eq!(Octave::Low.index(), 1);
    assert_eq!(Octave::High.index(), 7);
    assert_eq!(Tone::identify("loud"), Ok(Tone::Loud));
    assert_eq!(Tone::identify(" Loud"), Err(Fault::Invalid));
    assert_eq!(Tone::Quiet.speaker(), "quiet");
    let station=Station::make(2,Tone::Quiet,"AB").unwrap();
    assert_eq!((station.number(),station.tone(),station.code()),(2,Tone::Quiet,"AB"));
    assert_eq!(Station::make(3,Tone::Quiet,"AB"),Err(Fault::Invalid));
    assert_eq!(Station::make(2,Tone::Loud,"Ab"),Err(Fault::Invalid));
    assert_eq!(Station::make(2,Tone::Loud,"AAA"),Err(Fault::Invalid));
    let set=std::collections::HashSet::from([station.clone()]);
    assert!(set.contains(&station));
    let value=Metadata { station, caption:Caption::make("test").unwrap() };
    assert_eq!(value.caption.text(),"test");
    assert_eq!(format!("{:?}",value.caption),"Caption(\"test\")");
    assert_eq!(Fault::Invalid.kind(),"invalid");
    assert_eq!(Fault::Invalid.to_string(),"bad detector input");
    for text in ["ééé","éé","", "éééx", "x\u{009f}", "x\u{2066}"] {
        println!("{}",Caption::make(text).is_ok());
    }
}
"""


def run(*args, **kwargs):
    return subprocess.check_output(args,text=True,**kwargs)


def pin(path,root):
    return dict(path=str(path.relative_to(root)),sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def shared(root):
    type_id=n("fault")
    graph=dict(format="oddlang-shared-declarations-v1",id=n("faults"),
               errors=[dict(id=type_id,variants=[dict(id=FAULT,payload="none",
                    category="invalid",message=[{"text":"bad detector input"}])])])
    bind=dict(format="oddlang-shared-rust-v1",names={type_id:"Fault",FAULT:"Invalid"},
              variantOrder={type_id:[FAULT]},errorCategoryMethods={type_id:"kind"})
    return write(root/"errors.json",graph),write(root/"error-rust.json",bind)


def assembled(root,cli,g,r,tag):
    g=write(root/(tag+".json"),g)
    r=write(root/(tag+"-rust.json"),r)
    faults,names=shared(root)
    lock=root/"Cargo.lock"
    lock.write_text('version = 4\n\n[[package]]\nname = "detector"\nversion = "0.0.0"\n')
    package=write(root/(tag+"-package.json"),dict(format="oddlang-native-package-v1",name="detector",
        version="0.0.0",edition="2024",cargoLock=pin(lock,root),components=[
            dict(kind="shared-declarations",module="faults",graph=pin(faults,root),overlay=pin(names,root),reexports=["Fault"]),
            dict(kind="checked-values",module="values",graph=pin(g,root),overlay=pin(r,root),
                 reexports=["Caption","Metadata","Octave","Station","Tone"])]))
    out=root/tag
    run(cli,"build","native-package","rust",package,out)
    run("cargo","+1.97.1","build","--locked","--offline","--manifest-path",out/"Cargo.toml")
    source=root/"witness.rs"
    source.write_text(CALLER)
    exe=root/"witness"
    run("rustc","+1.97.1","--edition=2024",source,"--extern",f"detector={out/'target/debug/libdetector.rlib'}","-o",exe)
    return run(exe).splitlines()


def main():
    cli=argparse.ArgumentParser(description=__doc__)
    cli.add_argument("oddlang",type=Path)
    exe=cli.parse_args().oddlang.resolve()
    with tempfile.TemporaryDirectory() as work:
        root=Path(work)
        g,r=fixture()
        assert assembled(root,exe,g,r,"baseline")==["true","true","false","false","false","false"]
        original_rust=write(root/"deterministic-rust.json",r)
        graph=write(root/"deterministic.json",g)
        run(exe,"build","checked-values","rust",graph,original_rust,root/"first")
        r["types"][n("tone")]["traits"].reverse()
        write(original_rust,r)
        run(exe,"build","checked-values","rust",graph,original_rust,root/"second")
        for name in ("values.rs","obligations.json","projection.lock"):
            assert (root/"first"/name).read_bytes()==(root/"second"/name).read_bytes(),name
        lock=json.loads((root/"first/projection.lock").read_bytes())
        obligations_path=root/"first/obligations.json"
        assert lock["obligationsSha256"]==hashlib.sha256(obligations_path.read_bytes()).hexdigest()
        assert lock["sourceSha256"]==hashlib.sha256((root/"first/values.rs").read_bytes()).hexdigest()
        obligation={entry["declaration"]:entry["expected"] for entry in json.loads(obligations_path.read_bytes())["obligations"]}
        assert obligation[n("caption")]["orderedGuards"]==g["types"][n("caption")]["guards"]
        assert obligation[n("tone")]["values"]==g["types"][n("tone")]["values"]
        g,r=fixture(5)
        assert assembled(root,exe,g,r,"mutated")==["false","true","false","false","false","false"]
        for change,message in [
            ("cycle","recursive"),
            ("public","invalid checked field"),
            ("inaccessible","invalid checked record"),
            ("trait-shadow","invalid finite value table bindings"),
            ("mixed","invalid or repeated representation"),
            ("overlap","ill-typed constructor guard"),
        ]:
            g,r=fixture()
            if change=="cycle":
                g["types"][n("metadata")]["fields"][n("metadata:station")]["id"]=n("metadata")
            elif change=="public":
                r["types"][n("station")]["members"][n("station:code")]["public"]=True
            elif change=="inaccessible":
                r["types"][n("metadata")]["members"][n("metadata:caption")]["public"]=False
            elif change=="trait-shadow":
                r["types"][n("tone")]["view"]="clone"
            elif change=="mixed":
                g["types"][n("octave")]["values"][n("low")]="one"
            else:
                g["types"][n("station")]["guards"][-1]["ranges"].append(dict(minimum=90,maximum=95))
            dest=root/"rejected"
            result=subprocess.run([exe,"build","checked-values","rust",write(root/"graph.json",g),
                        write(root/"rust.json",r),dest],capture_output=True,text=True)
            assert result.returncode and message in result.stderr and not dest.exists(),result.stderr
    print("Qualified unrelated checked records, enum tables, error categories and a native bound mutation.")


if __name__=="__main__":
    main()
