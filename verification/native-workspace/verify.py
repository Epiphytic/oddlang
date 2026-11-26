"""Two independent native packages, their local dependency edge, and stale-pin controls."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI=Path(sys.argv[1]).resolve()


def run(*argv,cwd=None,ok=True):
    p=subprocess.run(argv,cwd=cwd,text=True,capture_output=True)
    if (p.returncode==0)!=ok: raise AssertionError(p.stderr)
    return p


def put(file,value):
    file.write_text(json.dumps(value,indent=2)+"\n")
    return pin(file)


def pin(p):
    return dict(path=p.name,sha256=hashlib.sha256(p.read_bytes()).hexdigest())


def make(root,name,constant,value):
    ident=f"urn:meter:{constant}"
    graph=put(root/f"{name}.json",dict(format="oddlang-shared-declarations-v1",id=f"urn:meter:{name}",
        constants=[dict(id=ident,value=value)]))
    overlay=put(root/f"{name}-rust.json",dict(format="oddlang-shared-rust-v1",names={ident:constant},constantTypes={ident:"u32"}))
    lock=root/f"{name}.lock";lock.write_text(f'version = 4\n[[package]]\nname="{name}"\nversion="0.1.0"\n')
    component=dict(kind="shared-declarations",module="values",graph=graph,overlay=overlay,reexports=[constant])
    spec=root/f"{name}-package.json"
    return put(spec,dict(format="oddlang-native-package-v1",name=name,version="0.1.0",edition="2024",dependencies={},
                         cargoLock=pin(lock),components=[component]))


with tempfile.TemporaryDirectory() as temp:
    root=Path(temp); first=make(root,"thermometer","LIMIT",20);second=make(root,"station","PRESSURE",90)
    lock=root/"Cargo.lock";lock.write_text('version=4\n[[package]]\nname="thermometer"\nversion="0.1.0"\n[[package]]\nname="station"\nversion="0.1.0"\ndependencies=["thermometer"]\n')
    spec=root/"workspace.json"
    m=dict(format="oddlang-native-workspace-v1",cargoLock=pin(lock),members={
        "thermometer":dict(manifest=first),"station":dict(manifest=second,dependencies=["thermometer"])})
    put(spec,m)
    def build(dest,ok=True): return run(CLI,"build","native-workspace","rust",spec,root/dest,ok=ok)
    build("generated");build("repeat")
    a=root/"generated";b=root/"repeat"
    def inventory(d):return {p.relative_to(d):p.read_bytes() for p in d.rglob("*") if p.is_file()}
    assert inventory(a)==inventory(b)
    run("cargo","test","--workspace","--locked","--offline",cwd=a)
    test=a/"station/tests";test.mkdir();(test/"consumer.rs").write_text(
        "#[test] fn permitted_values(){assert_eq!(station::PRESSURE,90);assert_eq!(thermometer::LIMIT,20);}")
    run("cargo","test","--workspace","--offline","--locked",cwd=a)
    assert "already exists" in build("generated",False).stderr
    for value,reason in [("station","self workspace edge"),("unknown","unresolved")]:
        m["members"]["station"]["dependencies"]=[value];put(spec,m)
        assert reason in build("missing",False).stderr and not (root/"missing").exists()
    m["members"]["station"]["dependencies"]=["thermometer"]
    m["members"]["thermometer"]["dependencies"]=["station"];put(spec,m)
    assert "cyclic" in build("cycle",False).stderr
    m["members"]["thermometer"].pop("dependencies");m["members"]["station"]["manifest"]["sha256"]="0"*64;put(spec,m)
    assert "digest mismatch" in build("tampered",False).stderr
    print("two checked packages; local edge linked by unchanged caller; fresh-output, cycle, identity and digest controls passed")

    from bootstrap import verify as verify_bootstrap
    verify_bootstrap(CLI, root / "bootstrap", make, pin, put)
    print("empty-cache failure, pinned Git acquisition, disconnected rebuild and stale root lock checked")
