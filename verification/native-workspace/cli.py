"""Execute the generated process binary through both package and workspace entrypoints."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

cli=Path(sys.argv[1]).resolve()
fixture=Path(__file__).resolve().parents[1]/"command-surface"


def run(*cmd,cwd=None):
    return subprocess.run(cmd,cwd=cwd,capture_output=True,stdin=subprocess.DEVNULL)


def check(*cmd,cwd=None):
    proc=run(*cmd,cwd=cwd)
    if proc.returncode: raise ValueError(proc.stderr.decode())
    return proc


with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp)
    src=root/"original";(src/"src").mkdir(parents=True)
    for name in ["Cargo.toml","Cargo.lock"]:shutil.copy(fixture/name,src/name)
    shutil.copy(fixture/"src/bin/reference.rs",src/"src/main.rs")
    check("cargo","build","--offline","--locked",cwd=src)
    check(cli,"recover","cli-surface",src/"src/main.rs",fixture/"selection.json",root/"graph.json")
    shutil.copy(fixture/"binding.json",root);shutil.copy(fixture/"Cargo.lock",root)
    def pin(n):return {"path":n,"sha256":hashlib.sha256((root/n).read_bytes()).hexdigest()}
    m=dict(format="oddlang-native-package-v1",name="observatory",edition="2021",version="2.1.0",
           dependencies={"clap":{"version":"4.6.7","features":["derive"]}},cargoLock=pin("Cargo.lock"),
           binaries=[dict(name="observatory",entry=["run_cli"])],
           components=[dict(kind="cli-surface",module="command",graph=pin("graph.json"),overlay=pin("binding.json"),reexports=["run_cli"])])
    p=root/"package.json";p.write_text(json.dumps(m))
    w=root/"workspace.json";w.write_text(json.dumps(dict(format="oddlang-native-workspace-v1",cargoLock=pin("Cargo.lock"),
                                                       members={"observatory":dict(manifest=pin("package.json"))})))
    for kind,spec in [("native-package",p),("native-workspace",w)]:
        out=root/kind;check(cli,"build",kind,"rust",spec,out)
        check("cargo","build","--bins","--locked","--offline",cwd=out)
        for args in [("--help",),("--version",),("--absent",),("help","capture")]:
            original=run(src/"target/debug/observatory",*args); emitted=run(out/"target/debug/observatory",*args)
            assert (original.returncode,original.stdout,original.stderr)==(emitted.returncode,emitted.stdout,emitted.stderr)
    m["binaries"][0]["entry"]=["missing_entry"];p.write_text(json.dumps(m))
    ref=json.loads(w.read_bytes());ref["members"]["observatory"]["manifest"]=pin("package.json");w.write_text(json.dumps(ref))
    bad=root/"bad"
    assert b"invalid workspace binary entry binding" in run(cli,"build","native-workspace","rust",w,bad).stderr and not bad.exists()
    print("standalone and composed binaries match the original on stdout, stderr and status; invalid workspace entry rejected")
