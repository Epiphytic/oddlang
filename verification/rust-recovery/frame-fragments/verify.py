"""Exercise compiler-resolved frame precheck and fragment layouts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
SELECT = "crate::Envelope::encode#Head,Copy"


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs)


def observe(temp, host):
    raw = run("bash", HERE.parent/"run.sh", temp/"Cargo.toml",
              "parts", "lib", "parts", host, "default",
              env=dict(os.environ, ODD_RECOVERY_TAGGED_FRAME_FRAGMENTS=SELECT))
    value = json.loads(raw)
    assert value["format"] == "oddlang-rust-frame-fragments-v1"
    return raw, value


D = "urn:parts:"
CALLER = """
fn main() {
    let head = |version| Envelope::Head(Head { first: [0x01, 0x62], second: [0x31, 0x9b], version }, Level::High);
    let copy = |count| Envelope::Copy(Copy { first: [0x71, 0xf2], second: [0x33, 0x2b], count });
    for frame in [head(1), head(2), copy(None), copy(Some(3)), copy(Some(2))] {
        match frame.encode() {
            Ok(data) => { for byte in data { print!("{byte:02x}"); } println!(); }
            Err(fault) => println!("{fault:?}"),
        }
    }
}
"""


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def hash_file(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def expectation(mutated):
    head = ("319b0162" if mutated else "0162319b") + "0000000102"
    copy = "332b71f2" if mutated else "71f2332b"
    return ["0d00000009"+head, "Invalid", "0e00000005"+copy+"00",
            "0e00000009"+copy+"0100000003", "Invalid"]


def record(temp, cli, variant, source, evidence, receipt, out):
    boundary = {key: value for key,value in receipt["cases"][variant].items() if key!="slots"}
    selection = out/(variant+".selection.json")
    write(selection, dict(format="oddlang-frame-fragment-selection-v1",
                          method=SELECT.split("#")[0], variant=variant, boundary=boundary,
                          sourceSha256=hash_file(source), evidenceSha256=hash_file(evidence),
                          validationOrder=[D+"version"] if variant=="Head" else []))
    fixed = dict(kind="fixed-bytes", size=2)
    version = {"kind":"unsigned-be","bits":32,"minimum":1,"maximum":1}
    optional = dict(kind="optional",payload=dict(kind="unsigned-be",bits=32),error=D+"invalid")
    layout = {"first":fixed, "second":fixed} | ({"version":version} if variant=="Head" else {"count":optional})
    graph = dict(format="oddlang-binary-record-v1", id=D+variant, maximum=64, fragment=True,
                 errors=dict(value=D+"invalid",decodeSize=D+"limit",encodeSize=D+"limit",
                             trailing=D+"invalid",truncated=D+"invalid"),
                 layout=[dict(id=D+name,encoding=encoding) for name,encoding in sorted(layout.items())])
    if variant=="Copy":
        graph["guards"]=[dict(id=D+"odd",error=D+"invalid",condition=dict(kind="optional-mask",
                         optional=D+"count", allowed=2**32-1, required=1))]
    overlay = dict(format="oddlang-binary-record-rust-v1", name=variant, errorType="crate::Fault",
                   names={D+name:name for name in layout}, externals={},
                   errors={D+"invalid":dict(variant="Invalid"), D+"limit":dict(variant="Limit")})
    paths = [out / (variant + suffix) for suffix in (".template.json",".rust.json","-recovered")]
    write(paths[0], graph)
    write(paths[1], overlay)
    run(cli,"recover","frame-fragment",temp,source,evidence,selection,*paths)
    target = out/variant
    obligations=json.loads((paths[2]/"dependencies.json").read_bytes())
    assert obligations["remainingIndependentObligations"]==["validator-semantics","validation-order","decoder"]
    run(cli,"build","binary-record","rust",paths[2]/"graph.json",paths[1],target)
    return target/"record.rs"


def frame(cli,out):
    level,head,copy=(D+n for n in ("Level","Head","Copy"))
    invalid,limit=D+"invalid",D+"limit"
    cases = {
        head:dict(tag=13,form="tuple",slots=[
            dict(id=D+"head-value",encoding=dict(kind="fragment",id=D+"head-fragment")),
            dict(id=D+"level-value",encoding=dict(kind="enum",id=level,error=invalid))]),
        copy:dict(tag=14,form="tuple",slots=[
            dict(id=D+"copy-value",encoding=dict(kind="fragment",id=D+"copy-fragment"))]),
    }
    tags={D+"low":1,D+"high":2}
    graph = dict(format="oddlang-binary-frame-v1",id=D+"frames",maximum=64,cases=cases,
                 errors={key:limit if key=="limit" else invalid for key in
                         ("header","length","limit","tag","truncated","trailing","decoderFailed")},
                 types={level:dict(kind="enum",tags=tags), D+"head-fragment":dict(kind="fragment"),
                        D+"copy-fragment":dict(kind="fragment")})
    overlay = dict(format="oddlang-binary-frame-rust-v1",name="Envelope",decoder="EnvelopeDecoder",
        errorType="crate::Fault",errors={invalid:dict(variant="Invalid"),limit:dict(variant="Limit")},
        cases={head:"Head",copy:"Copy"}, fields={D+n:n.replace("-","_") for n in ("head-value","level-value","copy-value")},
        types={level:dict(path="crate::Level", variants={D+"low":"Low",D+"high":"High"}),
               D+"head-fragment":dict(path="crate::Head"), D+"copy-fragment":dict(path="crate::Copy")})
    write(out/"frame.json", graph)
    write(out/"frame-rust.json", overlay)
    run(cli,"build","binary-frame","rust",out/"frame.json",out/"frame-rust.json",out/"frame")
    generated,=(out/"frame").glob("*.rs")
    return generated


def native(temp, code, expected):
    source=temp/"witness.rs"
    executable=temp/"witness"
    source.write_text(code+CALLER)
    run("rustc","+1.97.1","--edition=2024",source,"-o",executable)
    assert run(executable).splitlines()==expected


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("oddlang",type=Path)
    cli=parser.parse_args().oddlang.resolve()
    rust = run("rustc", "+1.97.1", "-vV")
    host = next(line.split()[1] for line in rust.splitlines() if line.startswith("host:"))
    with tempfile.TemporaryDirectory() as directory:
        temp = Path(directory)
        (temp/"src").mkdir()
        (temp/"Cargo.toml").write_text('[package]\nname="parts"\nversion="0.1.0"\nedition="2024"\n')
        source=temp/"src/lib.rs"
        original=(HERE/"fixture.rs").read_text()
        source.write_text(original)
        run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=temp)
        altered=original
        for field in ("head","copy"):
            first=f"    out.extend({field}.first);\n"
            second=f"    out.extend({field}.second);\n"
            assert altered.count(first+second)==1
            altered=altered.replace(first+second,second+first)
        for label,text in [("baseline",original),("mutant",altered)]:
            source.write_text(text)
            raw,value=observe(temp,host)
            assert not value["unresolved"], value
            assert list(value["cases"])==["Copy","Head"]
            out=temp/label
            out.mkdir()
            receipt=out/"compiler.json"
            receipt.write_text(raw)
            code=["#[derive(Debug)] pub enum Fault { Invalid, Limit }",
                  "#[derive(Clone,Copy,PartialEq,Eq)] pub enum Level { Low, High }"]
            for case in ("Head","Copy"):
                artifact=record(temp,cli,case,source,receipt,value,out)
                code.append(f"mod module_{case.lower()} {{ {artifact.read_text()} }}")
                code.append(f"use module_{case.lower()}::{case};")
            code.append(frame(cli,out).read_text())
            expected=expectation(label=="mutant")
            native(temp,text,expected)
            native(temp,"\n".join(code),expected)
        partial = original.replace("count: Option<u32>", "count: Option<Metadata>").replace(
            "count & 1 == 0","count.first & 1 == 0"
        ).replace("marked(out, copy.count, append32);",
                  "marked(out, copy.count, |out, value| out.extend(value.first.to_be_bytes()));"
        ) + "\n#[derive(Clone, Copy)] pub struct Metadata { pub first:u64, pub second:u32 }\n"
        for text in (
            partial,
            original.replace("        self.validate()?;\n",""),
            original.replace("                head.validate()?;\n",""),
            original.replace("out.push(0),","out.push(2),"),
            original.replace("                append_head(&mut payload, head);","                payload.clear(); append_head(&mut payload, head);"),
        ):
            source.write_text(text)
            _, rejected=observe(temp,host)
            assert rejected["unresolved"] and "cases" not in rejected, rejected
    print("Verified checked append and checked delegate layouts, native bytes and rejections.")


if __name__ == "__main__":
    main()
