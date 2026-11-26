"""Verify compiler-resolved tagged-frame writes against an unrelated native encoder."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
METHOD = "crate::Packet::encode"


def run(*args, **kw):
    return subprocess.check_output(args, text=True, **kw)


def compiler_observation(project, host):
    raw = run("bash", HERE.parent / "run.sh", project / "Cargo.toml", "packet", "lib",
              "packet", host, "default",
              env=dict(os.environ, ODD_RECOVERY_TAGGED_FRAME=METHOD))
    receipt = json.loads(raw)
    assert receipt.get("format") == "oddlang-rust-tagged-frame-v1", receipt.get("format")
    return raw, receipt


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def scaffold(temp):
    domain = "urn:packet:"
    fields = ["Key:first", "Key:second", "Reading:footer", "Reading:value"]
    errors = {role: domain + role for role in
              ("header", "length", "limit", "tag", "truncated", "trailing", "decoderFailed")}
    fixed = {"kind": "fixed", "size": 2}
    unsigned = {"kind": "unsigned", "bits": 64, "maximum": 2**64 - 1, "error": errors["limit"]}
    literal = {"kind": "literal", "value": 0, "error": errors["truncated"]}
    cases = {
        "Key": {"first": fixed, "second": fixed},
        "Reading": {"footer": fixed, "stamp": literal, "value": unsigned},
    }
    template = dict(format="oddlang-binary-frame-v1", id=domain+"graph", maximum=0,
                    errors=errors, types={}, cases={
       domain + name: dict(form="named", tag=0,
                           slots=[dict(id=domain+name+":"+field, encoding=encoding)
                                  for field, encoding in sorted(slots.items())])
       for name, slots in cases.items()})
    overlay = dict(format="oddlang-binary-frame-rust-v1", name="Packet", decoder="PacketDecoder",
                   errorType="crate::Fault",
                   errors={id: dict(variant=label[0].upper()+label[1:]) for label, id in errors.items()},
                   cases={domain+name: name for name in cases},
                   fields={domain+full: full.split(":")[1] for full in fields}, types={})
    write_json(temp/"template.json", template)
    write_json(temp/"rust.json", overlay)


CALLER = """
fn main() {
    for packet in [Packet::Key { first: [0x41, 0x91], second: [0x33, 0x66] },
                   Packet::Reading { value: 0x0102030405060708, footer: [0x0f, 0x7e] }] {
        for byte in packet.encode().unwrap() { print!("{byte:02x}"); }
        println!();
    }
}
"""
SUPPORT = """
#[derive(Debug)]
pub enum Fault { Header, Length, Limit, Tag, Truncated, Trailing, DecoderFailed }
"""


def native(temp, candidate, expected):
    source = temp / "witness.rs"
    source.write_text(candidate + CALLER)
    executable = temp / "witness"
    run("rustc", "+1.97.1", "--edition=2024", source, "-o", executable)
    assert run(executable).splitlines() == expected


def qualify(temp, source, cli, name, text, receipt, expected):
    evidence = temp / (name+".receipt.json")
    selection = temp / (name+".selection.json")
    recovery = temp / (name+"-recovery")
    graph = recovery / "graph.json"
    evidence.write_text(receipt)
    write_json(selection, dict(format="oddlang-tagged-frame-selection-v1", method=METHOD,
                     sourceSha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                     evidenceSha256=hashlib.sha256(evidence.read_bytes()).hexdigest(),
                     tupleSlots={}, fragmentHelpers={}, acceptedUnresolved={}))
    run(cli,"recover","tagged-frame",source.parents[1],source,evidence,selection,
        temp/"template.json",temp/"rust.json",recovery)
    assert json.loads((recovery/"obligations.json").read_bytes())["unresolved"] == {}
    generated = temp / name
    run(cli,"build","binary-frame","rust",graph,temp/"rust.json",generated)
    implementation, = generated.glob("*.rs")
    native(temp, text, expected)
    native(temp, SUPPORT+implementation.read_text(), expected)
    return graph


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("oddlang", type=Path)
    cli = parser.parse_args().oddlang.resolve()
    host = next(line.split()[1] for line in run("rustc", "+1.97.1", "-vV").splitlines()
                if line.startswith("host:"))
    with tempfile.TemporaryDirectory() as directory:
        project = Path(directory)
        (project / "src").mkdir()
        (project / "Cargo.toml").write_text('[package]\nname="packet"\nversion="0.1.0"\nedition="2024"\n')
        source = project / "src/lib.rs"
        text = (HERE / "fixture.rs").read_text()
        source.write_text(text)
        run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=project)
        scaffold(project)
        raw, baseline = compiler_observation(project, host)
        cases = {case["variant"]: case for case in baseline["cases"]}
        assert not baseline["unresolved"], baseline
        assert cases["Key"]["tag"] == 31 and cases["Reading"]["tag"] == 32
        assert [slot["field"] for slot in cases["Key"]["writes"]] == ["first", "second"]
        assert [slot.get("field") for slot in cases["Reading"]["writes"]] == ["value", "footer", None]
        assert cases["Reading"]["writes"][-1]["literal"] == 84
        base_reading = "200000000b01020304050607080f7e54"
        before = qualify(project, source, cli, "baseline", text, raw,
                         ["1f0000000441913366", base_reading])
        altered = text.replace("                31\n", "                34\n", 1).replace(
            "                payload.extend(a);\n                payload.extend(second);",
            "                payload.extend(second);\n                payload.extend(a);", 1)
        source.write_text(altered)
        raw, mutation = compiler_observation(project, host)
        changed = {case["variant"]: case for case in mutation["cases"]}["Key"]
        assert changed["tag"] == 34 and [slot["field"] for slot in changed["writes"]] == ["second", "first"]
        after = qualify(project, source, cli, "mutant", altered, raw,
                        ["220000000433664191", base_reading])
        assert before.read_bytes() != after.read_bytes()
        clearing_writer = text.replace(
            "Reading { value: u64, footer: [u8; 2] },", "Reading { item: Record },"
        ).replace("Self::Reading { value, footer }", "Self::Reading { item }"
        ).replace("append_reading(&mut payload, *value, footer);", "clear_record(&mut payload, item);"
        ) + """
pub struct Record;
fn clear_record(out: &mut Vec<u8>, _record: &Record) { out.clear(); }
"""
        trait_dispatch = text.replace("Reading { value: u64, footer: [u8; 2] },",
            "Reading { value: u64, footer: [u8; 2], signal: Signal },"
        ).replace("Self::Reading { value, footer }", "Self::Reading { value, footer, signal }"
        ).replace("payload.push(84);", "payload.push(Signal::number(*signal));") + """
#[derive(Clone, Copy)]
pub enum Signal { Ready }
trait Number {
    fn number(self) -> u8;
}
impl Number for Signal {
    fn number(self) -> u8 { match self { Signal::Ready => 53 } }
}
"""
        for text_control, case, expected in (
            (text.replace("bytes.push(tag);", "bytes.push(tag ^ 1);"), None, "selected discriminator"),
            (text.replace("                32\n", "                31\n"), None, "collision"),
            (text.replace("payload.extend(a);", "payload.extend(a); payload.clear();"), "Key", "unknown write"),
            (text.replace("data.to_be_bytes()", "data.to_le_bytes()"), "Reading", "matched field"),
            (clearing_writer, "Reading", "mutate or escape"),
            (trait_dispatch, "Reading", "proven enum mapping"),
            (text.replace("payload.extend(a);", "let aliased = &mut payload; aliased.extend(a);"),
             "Key", "local alias"),
        ):
            source.write_text(text_control)
            _, observed = compiler_observation(project, host)
            row = observed if case is None else next(row for row in observed["cases"] if row["variant"] == case)
            assert expected in " ".join(row["unresolved"]), row
            assert "writes" not in row, row
    print("Verified compiler-resolved tagged-frame discriminator, writes, and altered header control.")


if __name__ == "__main__":
    main()
