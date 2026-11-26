"""Exercise a pinned declaration export against independent source revisions."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI=Path(sys.argv[1]).resolve()
SOURCE="""
pub const CAP: u16 = 3 * 17;
pub const SOFT: u16 = CAP;
#[derive(Debug, thiserror::Error)]
pub enum Alarm {
    #[error("sensor {{limit}}: {0}")]
    Reading(u64),
    #[error("no signal")]
    Missing,
}
pub type ReadingResult<T> = core::result::Result<T, Alarm>;
"""
def run(*args, cwd=None, ok=True):
    proc=subprocess.run(args,cwd=cwd,capture_output=True,text=True)
    if (proc.returncode==0)!=ok: raise RuntimeError(proc.stderr)
    return proc.stdout.strip()
with tempfile.TemporaryDirectory() as tmp:
    work=Path(tmp)
    root=work/"sensor";root.mkdir()
    run("git","init","-q",cwd=root)
    decl=work/"selection.json"
    cases=[
        (SOURCE,True),(SOURCE.replace("3 * 17","4 * 19").replace("no signal","disconnected"),True),
        (SOURCE.replace("pub const CAP","#[cfg(unix)] pub const CAP"),False),
        (SOURCE.replace("{0}","{0:?}"),False),
        (SOURCE.replace("SOFT: u16 = CAP","SOFT: u16 = unknown()"),False),
        (SOURCE.replace("Reading(u64)","Reading(u64,u64)"),False),
    ]
    for n,(src,ok) in enumerate(cases):
        file=root/"lib.rs";file.write_text(src)
        run("git","add","lib.rs",cwd=root)
        run("git","-c","user.name=test","-c","user.email=test@example.invalid","commit","-qm","fixture",cwd=root)
        revision=run("git","rev-parse","HEAD",cwd=root)
        file.write_text("working tree deliberately differs")
        decl.write_text(json.dumps(dict(sourceRevision=revision,moduleFile="lib.rs",domain="urn:t:values",
            constants={"CAP":"urn:t:cap","SOFT":"urn:t:soft"},
            errors={"Alarm":{"id":"urn:t:alarm","variants":{"Reading":"urn:t:reading","Missing":"urn:t:missing"}}},
            results={"ReadingResult":"urn:t:result"})))
        out=work/str(n)
        cmd=(CLI,"recover","shared-declarations",root,decl,out)
        run(*cmd,ok=ok)
        assert out.exists()==ok
        if ok:
            g=json.loads((out/"graph.json").read_bytes())
            assert g["constants"][0]["value"]=={"multiply":[4,19] if n else [3,17]}
            assert g["constants"][1]["value"]=={"ref":"urn:t:cap"}
            variants=g["errors"][0]["variants"]
            assert variants[0]["message"]==[{"text":"sensor {limit}: "},{"payload":True}]
            assert variants[1]["message"]==[{"text":"disconnected" if n else "no signal"}]
            assert json.loads((out/"observation.json").read_bytes())["sourceRevision"]==revision
            run(*cmd,ok=False)
    print("pinned revisions, symbolic references, typed messages, unsupported cases and existing destinations checked")
