"""Check a provenance command through the assembled Cargo binary."""
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
OP = "urn:observatory:provenance"
REV = "af" * 20
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1", CARGO_NET_OFFLINE="true")


def run(*args, cwd, env=ENV, check=True):
    return subprocess.run(args, cwd=cwd, env=env, check=check, capture_output=True)


def pin(path):
    return {"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def write(root, name, value):
    path = root / name
    path.write_text(json.dumps(value))
    return pin(path)


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    cargo = root / "Cargo.toml"
    (root / "src").mkdir()
    (root / "src/lib.rs").touch()
    deps = {"clap": {"version": "4.6.7"}, "serde_json": {"version": "1.0.154"}}
    cargo.write_text('[package]\nname="observatory"\nversion="2.1.0"\nedition="2021"\n[dependencies]\n'
                     + "".join(f'{name}="={spec["version"]}"\n' for name,spec in deps.items()))
    run("cargo", "generate-lockfile", "--offline", cwd=root)
    def field(key, kind, **params):
        return dict(id="urn:obs:field:"+key, wire=key, source=dict(kind=kind, **params))

    graph = dict(format="oddlang-build-report-v1", id=OP, fields=[
        field("edition", "unsigned", bits=8, value=2), field("api", "package-version"),
        field("target", "build-target"), field("source", "revision", value=REV),
        field("formats", "unsigned-list", bits=32,length=3,value=[0, 2, 65536])])
    surface = dict(profile="command-surface-v1", package_name="observatory", version="2.1.0",
                   root=dict(name="observatory", subcommands=[dict(name="about")]))
    adapter = dict(executable="observatory", invalid_exit=2, uncovered_exit=125,
                   operations={"about":OP}, handlers={OP:["report","show"]})
    component = dict(kind="build-report", module="report", reexports=["show"],
                     graph=write(root,"report.json",graph),overlay=write(root,"native.json",dict(entry="show")))
    package = dict(format="oddlang-native-package-v1", name="observatory", version="2.1.0", edition="2021",
                   cargoLock=pin(root/"Cargo.lock"), dependencies=deps,
                   components=[component,dict(kind="cli-surface",module="cli",reexports=["run_cli"],
                               graph=write(root,"cli.json",surface),overlay=write(root,"cli-rust.json",adapter))],
                   binaries=[dict(name="observatory",entry=["run_cli"])])
    host = next(s[6:] for s in run("rustc","-vV",cwd=root).stdout.decode().splitlines() if s.startswith("host: "))
    def build(name, report, error=None):
        component["graph"] = write(root,"report.json",report)
        write(root,"package.json",package)
        out=root/name
        got=run(CLI,"build","native-package","rust",root/"package.json",out,cwd=root,check=False)
        if error:
            assert got.returncode and error in got.stderr.decode() and not out.exists(), got
            return
        assert got.returncode==0,got.stderr
        lock=json.loads((out/"package.lock").read_bytes())
        assert lock["generatedFiles"]["build.rs"] == hashlib.sha256((out/"build.rs").read_bytes()).hexdigest()
        run("cargo","build","--locked","--offline",cwd=out)
        got=run(out/"target/debug/observatory","about",cwd=root,env=dict(ENV,TARGET="wrong",CARGO_PKG_VERSION="wrong"))
        return got.stdout,got.stderr
    expected={"edition":2,"api":"2.1.0","target":host,"source":REV,"formats":[0,2,65536]}
    def line(value):
        return json.dumps(value,separators=(",",":")).encode()+b"\n",b""
    assert build("product",graph)==line(expected)
    altered=deepcopy(graph)
    altered["fields"][0]["wire"]='wire"key'
    altered["fields"][3]["source"]["value"]="12"*20
    expected={'wire"key':2,**{k:v for k,v in expected.items() if k!="edition"}}
    expected["source"]="12"*20
    assert build("altered",altered)==line(expected)
    for key, bad, error in [("id",graph["fields"][1]["id"],"duplicate"),("wire","api","duplicate"),
                            ("source",dict(kind="revision",value="main"),"revision")]:
        altered=deepcopy(graph);altered["fields"][0][key]=bad
        build("invalid",altered,error)
    for source, error in (
        (dict(kind="unsigned", bits=8, value=256), "range"),
        (dict(kind="unsigned", bits=0, value=0), "width"),
        (dict(kind="unsigned-list", bits=32, length=2, value=[1]), "length"),
        (dict(kind="unsigned-list", bits=32, length=1, value=[2**32]), "range"),
    ):
        invalid=deepcopy(graph)
        invalid["fields"][0]["source"]=source
        build("invalid-width",invalid,error)

    args = [
        dict(id="hold", long="hold", global_=True),
        dict(id="delay", long="delay", kind="string", global_=True),
        dict(id="mode", long="mode", kind="string", global_=True),
        dict(id="path", long="path", kind="path", global_=True),
    ]
    for argument in args:
        argument["global"]=argument.pop("global_")
    surface["root"]["arguments"]=args
    package["components"][1]["graph"]=write(root,"cli.json",surface)
    guarded=deepcopy(graph)
    guarded["guards"]=[
        dict(condition="flag_false",argument="hold",rejected=dict(exit=4,stderr="hold")),
        dict(condition="absent",argument="delay",rejected=dict(exit=5,stderr="delay")),
        dict(condition="one_of",argument="mode",values=["safe"],rejected=dict(exit=6,stderr="mode")),
        dict(condition="path_present",argument="path",rejected=dict(exit=7,stderr="path")),
    ]
    guarded_pin=write(root,"report.json",guarded)
    component["graph"]=guarded_pin
    write(root,"package.json",package)
    out=root/"guarded"
    run(CLI,"build","native-package","rust",root/"package.json",out,cwd=root)
    run("cargo","build","--locked","--offline",cwd=out)
    exe=out/"target/debug/observatory"
    for argv,code,message in [
        (["--hold","--delay","1","--mode","bad"],4,"hold"),
        (["--delay","1","--mode","bad"],5,"delay"), (["--mode","bad"],6,"mode"),
        ([],7,"path"),
    ]:
        result=run(exe,"about",*argv,cwd=root,check=False)
        assert (result.returncode,result.stdout,result.stderr)==(code,b"",f"{message}\n".encode())
    expected["source"]=REV
    result=run(exe,"about","--mode","safe","--path","x",cwd=root)
    assert json.loads(result.stdout)=={"edition":2,"api":"2.1.0","target":host,"source":REV,"formats":[0,2,65536]}
    wrong=deepcopy(guarded)
    wrong["id"]="urn:observatory:other"
    build("wrong-operation",wrong,"operation identity differs")

    cargo.write_text(cargo.read_text().replace('version="2.1.0"','version="2.1.1"'))
    run("cargo","generate-lockfile","--offline",cwd=root)
    package["version"]="2.1.1"
    package["cargoLock"]=pin(root/"Cargo.lock")
    surface["version"]="2.1.1"
    package["components"][1]["graph"]=write(root,"cli.json",surface)
    stdout,stderr=build("new-package-version",graph)
    assert json.loads(stdout)["api"]=="2.1.1" and not stderr
print("Build report matches independent ordered JSON, native build facts and checked package inputs.")
