"""Compile an unrelated meter package from declarations, checking exact errors, order and scope."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parents[2] / "tests/fixtures/json-envelope"
ID = "urn:meter:"
def ref(name):
    return ID + name


def run(*args, error=None, cwd=None):
    result = subprocess.run(args, capture_output=True, text=True, cwd=cwd)
    if error:
        assert result.returncode and error in result.stderr, result.stderr
    else:
        assert result.returncode == 0, result.stderr


def write(path, data):
    path.write_text(json.dumps(data, sort_keys=True))
    return pin(path)


def pin(path):
    return {"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


graph = {
    "format":"oddlang-shared-declarations-v1","id":ref("domain"),
    "constants":[{"id":ref("step"),"value":17},
                 {"id":ref("period"),"value":{"multiply":[3,{"ref":ref("step")}]}}],
    "results":[{"id":ref("outcome"),"error":ref("fault")}],
    "errors":[{"id":ref("fault"),"variants":[
        {"id":ref("paused"),"payload":"none","message":[{"text":"paused"}]},
        {"id":ref("label"),"payload":"text","message":[{"text":"sensor {"},{"payload":True},{"text":"}"}]},
        {"id":ref("count"),"payload":"u64","message":[{"payload":True},{"text":" cycles"}]},
        {"id":ref("flag"),"payload":"bool","message":[{"payload":True}]}
    ]}],
    "stringEnums":[{"id":ref("mode"),"numbers":{"tags":{ref("cold"):3,ref("hot"):11},"unknown":{"id":ref("label"),"argument":"mode code"}},"variants":[{"id":ref("hot"),"wire":"h.ot"},{"id":ref("cold"),"wire":"COLD"}]}]
}
binding = {
    "format":"oddlang-shared-rust-v1",
    "names":{ref(k):v for k,v in dict(step="STEP",period="PERIOD",outcome="Reading",fault="SensorError",
             paused="Paused",label="Label",count="Count",flag="Flag",mode="Mode",hot="Hot",cold="Cold").items()},
    "constantTypes":{ref("step"):"usize",ref("period"):"u16"},
    "variantOrder":{ref("fault"):[ref(v) for v in ["label","count","flag","paused"]],
                    ref("mode"):[ref("cold"),ref("hot")]}
}
caller = r"""
use thumbnail_consumer::{PERIOD, Reading, SensorError};
use thumbnail_consumer::meter::Mode;
#[test]
fn inspect() {
    assert_eq!(PERIOD, 51u16);
    let a: Reading<u32> = Err(SensorError::Label("probe"));
    assert_eq!(a.unwrap_err().to_string(),"sensor {probe}");
    assert_eq!(format!("{:>20}", SensorError::Label("probe")), "sensor {probe}");
    assert_eq!(format!("{:.1}", SensorError::Label("probe")), "sensor {probe}");
    assert_eq!(format!("{:08}", SensorError::Count(42)), "42 cycles");
    assert_eq!(SensorError::Count(u64::MAX).to_string(), "18446744073709551615 cycles");
    assert_eq!(SensorError::Flag(true).to_string(), "true");
    assert_eq!(SensorError::Paused.to_string(), "paused");
    assert_eq!(format!("{:?}", std::mem::discriminant(&SensorError::Paused)), "Discriminant(3)");
    assert_eq!(Mode::Hot as u8,11);
    for b in 0u8..=255 {
        let expected = match b {3=>Ok(Mode::Cold),11=>Ok(Mode::Hot),_=>Err(SensorError::Label("mode code"))};
        assert_eq!(Mode::try_from(b),expected);
    }
    assert_eq!(serde_json::to_string(&Mode::Hot).unwrap(), r#""h.ot""#);
    assert_eq!(serde_json::from_str::<Mode>(r#""COLD""#).unwrap(), Mode::Cold);
    assert!(serde_json::from_str::<Mode>(r#""cold""#).is_err());
}
"""
with tempfile.TemporaryDirectory() as directory:
    root=Path(directory)
    g=root/"graph.json"
    b=root/"binding.json"
    spec=root/"package.json"
    def project(graph, binding, name, error=None):
        write(g,graph); write(b,binding)
        target=root/name
        run(CLI,"build","shared-declarations","rust",g,b,target,error=error)
        assert target.exists() == (error is None)
        return target
    good=project(graph,binding,"good")
    reversed_graph=copy.deepcopy(graph)
    reversed_graph["constants"].reverse()
    reversed_graph["errors"][0]["variants"].reverse()
    reversed_graph["stringEnums"][0]["variants"].reverse()
    replay=project(reversed_graph,binding,"replay")
    obligations = json.loads((good/"obligations.json").read_bytes())
    rows = obligations["declarations"]
    assert set(rows) == set(binding["names"])
    assert rows[ref("period")]["unsignedDecimal"] == "51"
    assert rows[ref("label")]["examples"][0] == {"payloadCanonicalText":"", "display":"sensor {}"}
    assert rows[ref("hot")]["wireString"] == "h.ot"
    assert rows[ref("mode")]["numberAdmission"]["display"] == "sensor {mode code}"
    assert len(rows[ref("mode")]["numberAdmission"]["reject"]) == 254
    outside=project(graph,dict(binding,externalErrorTypes={ref("fault"):"crate::provider::SensorError"}),"outside")
    external=(outside/"shared.rs").read_text()
    assert "pub enum SensorError" not in external and 'crate::provider::SensorError::Label("mode code")' in external

    assert rows[ref("mode")]["rejectWireString"] not in {"COLD","h.ot"}
    assert rows[ref("mode")]["nativeVariantOrder"] == [ref("cold"),ref("hot")]
    for f in ("shared.rs","projection.lock","obligations.json"):
        assert (good/f).read_bytes()==(replay/f).read_bytes()
    for name, update, error in [
        ("cycle",lambda g,b:g["constants"][0].update(value={"ref":ref("period")}),"cyclic constant"),
        ("overflow",lambda g,b:g["constants"][1].update(value={"multiply":[2**63,2]}),"product overflow"),
        ("width",lambda g,b:g["constants"][1].update(value=65536),"exceeds native type"),
        ("missing",lambda g,b:g["results"][0].update(error=ref("absent")),"unresolved result error"),
        ("message",lambda g,b:g["errors"][0]["variants"][0].update(message=[{"payload":True}]),"typed display"),
        ("order",lambda g,b:b["variantOrder"][ref("mode")].append(ref("cold")),"variant order"),
        ("tag",lambda g,b:g["stringEnums"][0]["numbers"]["tags"].update({ref("hot"):3}),"cover every variant uniquely"),
        ("constructor",lambda g,b:g["stringEnums"][0]["numbers"]["unknown"].update(id=ref("absent")),"unresolved number error"),
        ("constructor-payload",lambda g,b:g["stringEnums"][0]["numbers"]["unknown"].update(id=ref("count")),"number error payload"),
        ("wire",lambda g,b:g["stringEnums"][0]["variants"][1].update(wire="h.ot"),"wire spelling"),
        ("name",lambda g,b:b["names"].update({ref("mode"):"Bad; fn"}),"invalid Rust name"),
    ]:
        mutated, overlay=copy.deepcopy(graph),copy.deepcopy(binding)
        update(mutated,overlay)
        project(mutated,overlay,name,error)
    write(g,graph)
    for field in ("names","constantTypes","variantOrder"):
        raw=json.dumps(binding)
        key, value=next(iter(binding[field].items()))
        marker='"' + field + '": {'
        b.write_text(raw.replace(marker,marker+json.dumps(key)+':'+json.dumps(value)+',',1))
        run(CLI,"build","shared-declarations","rust",g,b,root/("duplicate-"+field),error="duplicate binding key")
    lock=root/"Cargo.lock";lock.write_bytes((FIXTURE/"Cargo.lock").read_bytes())
    component=dict(kind="shared-declarations",module="meter",graph=write(g,graph),overlay=write(b,binding),
                   visibility="public",reexports=["PERIOD","Reading","SensorError","Mode"],
                   rootReexports=["PERIOD","Reading","SensorError"],crateReexports=["STEP"])
    package=dict(format="oddlang-native-package-v1",name="thumbnail-consumer",version="0.0.0",edition="2021",
                 dependencies={"serde":{"version":"1.0.229","features":["derive"]},
                               "serde_json":{"version":"1.0.154"},"base64":{"version":"0.22.1"}},
                 cargoLock=pin(lock),components=[component])
    write(spec,package)
    out=root/"out"
    run(CLI,"build","native-package","rust",spec,out)
    assert "pub(crate) use" in (out/"src/group_0/mod.rs").read_text()
    tests=out/"tests";tests.mkdir()
    (tests/"oracle.rs").write_text(caller)
    run("cargo","+1.97.1","test","--locked","--offline","--quiet",cwd=out)
    for invalid,diagnostic in [("thumbnail_consumer::Mode","unresolved import"),("thumbnail_consumer::meter::STEP","private")]:
        (tests/"bad.rs").write_text(f"use {invalid};")
        run("cargo","+1.97.1","check","--locked","--offline","--test","bad",cwd=out,error=diagnostic)
print("Shared constants, errors, typed messages, aliases, native ordinals, serde wires and module visibility qualified.")
