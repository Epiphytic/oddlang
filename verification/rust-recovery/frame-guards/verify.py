"""Exercise compiler-proven frame guard priority against native Rust."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
METHOD = "crate::Parcel::encode"
CASE = "Chunk"


def run(*arguments, **kwargs):
    return subprocess.check_output(arguments, text=True, **kwargs)


def observer(project, host):
    value = run("bash", HERE.parent / "run.sh", project / "Cargo.toml", "parcel", "lib",
                "parcel", host, "default",
                env=dict(os.environ, ODD_RECOVERY_TAGGED_FRAME_GUARDS=METHOD+"#"+CASE))
    observed = json.loads(value)
    assert observed.get("format") == "oddlang-rust-frame-guards-v1", observed.get("format")
    return value, observed


IDENTITY = "urn:parcel:chunk"
RANGE = IDENTITY + ":end"
COUNT = IDENTITY + ":capacity"
FAULTS = {
    "header": "Header", "length": "Length", "limit": "Capacity", "tag": "Tag",
    "truncated": "Truncated", "trailing": "Trailing", "decoderFailed": "DecoderFailed",
}
CALLER = """
fn main() {
    for (seq, n) in [(10, 0), (10, 5), (0, 0), (9, 1), (8, 1), (u64::MAX, 1), (5, 5), (0, 2)] {
        match (Parcel::Chunk { seq, contents: vec![0x51; n] }).encode() {
            Ok(bytes) => { for byte in bytes { print!("{byte:02x}"); } println!(); }
            Err(error) => println!("{error:?}"),
        }
    }
}
"""


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def scaffold(project):
    errors = {name: "urn:parcel:"+value for name, value in FAULTS.items()}
    faults = {value: dict(variant=FAULTS[name]) for name, value in errors.items()}
    faults["urn:parcel:End"] = dict(variant="End")
    start = dict(kind="unsigned", bits=64, maximum=9, error="urn:parcel:End")
    rest = dict(kind="rest", minimum=1, maximum=4, error=errors["limit"])
    write_json(project/"graph.json", dict(format="oddlang-binary-frame-v1", id="urn:parcel:union",
              maximum=64, errors=errors, types={}, cases={IDENTITY:dict(tag=43, form="named",
              slots=[dict(id=IDENTITY+":seq", encoding=start),
                     dict(id=IDENTITY+":contents", encoding=rest)])}))
    write_json(project/"rust.json", dict(format="oddlang-binary-frame-rust-v1", name="Parcel",
              decoder="ParcelDecoder", errorType="crate::Fault", errors=faults, types={},
              cases={IDENTITY:"Chunk"}, fields={IDENTITY+":seq":"seq", IDENTITY+":contents":"contents"}))


def qualify(project, cli, source, raw, label, expected):
    evidence = project / (label+".evidence.json")
    selection = project / (label+".selection.json")
    graph = project / (label+".graph.json")
    evidence.write_text(raw)
    write_json(selection, dict(format="oddlang-frame-guard-selection-v1", method=METHOD,
              sourceSha256=sha(source), evidenceSha256=sha(evidence),
              cases={IDENTITY:{"count:contents":COUNT, "end-at-most:seq:contents":RANGE}}))
    run(cli, "recover", "frame-guards", project, source, evidence, selection,
        project/"graph.json", project/"rust.json", graph)
    out = project / label
    run(cli, "build", "binary-frame", "rust", graph, project/"rust.json", out)
    code, = out.glob("*.rs")
    preamble = source.read_text().split("pub enum Parcel",1)[0]
    for native in (source.read_text(), preamble + code.read_text()):
        file = project / "witness.rs"
        executable = project / "witness"
        file.write_text(native + CALLER)
        run("rustc", "+1.97.1", "--edition=2024", file, "-o", executable)
        assert run(executable).splitlines() == expected, label
    return [guard["id"] for guard in json.loads(graph.read_bytes())["cases"][IDENTITY]["encodeGuards"]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("oddlang", type=Path)
    cli = parser.parse_args().oddlang.resolve()
    host = next(line.split()[1] for line in run("rustc", "+1.97.1", "-vV").splitlines()
                if line.startswith("host:"))
    with tempfile.TemporaryDirectory() as directory:
        project = Path(directory)
        (project / "src").mkdir()
        (project / "Cargo.toml").write_text('[package]\nname="parcel"\nversion="0.1.0"\nedition="2024"\n')
        source = project / "src/lib.rs"
        text = (HERE / "fixture.rs").read_text()
        source.write_text(text)
        run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=project)
        scaffold(project)
        raw, observed = observer(project, host)
        assert not observed["unresolved"], observed
        guards = observed["cases"][CASE]["guards"]
        assert [rule["kind"] for rule in guards] == ["count", "end-at-most"], observed
        expected = ["Capacity", "Capacity", "Capacity", "End", "2b00000009000000000000000851",
                    "End", "Capacity", "2b0000000a00000000000000005151"]
        assert qualify(project, cli, source, raw, "baseline", expected) == [COUNT, RANGE]
        count = """                if contents.is_empty() || contents.len() > CHUNK {
                    return Err(Fault::Capacity);
                }
"""
        end = "                upper(*number, contents.len())?;\n"
        assert text.count(count+end) == 1
        source.write_text(text.replace(count+end, end+count))
        raw, observed = observer(project, host)
        assert not observed["unresolved"], observed
        for index in (0, 1, 6):
            expected[index] = "End"
        assert qualify(project, cli, source, raw, "mutated", expected) == [RANGE, COUNT]
        for variant, obligation in (
            (text.replace("sequence.checked_add", 'eprintln!("effect"); sequence.checked_add'), "unproven statements"),
            (text.replace("                payload.extend((*number).to_be_bytes());\n", "")
                 .replace(count+end, count+"                payload.extend((*number).to_be_bytes());\n"+end),
             "interleave"),
            (text.replace("Result<u64, Fault> {\n    sequence.checked_add", "Result<u64, Different> {\n    sequence.checked_add")
                 .replace(".ok_or(Fault::End)", ".ok_or(Different::Failure)") + """
enum Different { Failure }
impl From<Different> for Fault {
    fn from(_: Different) -> Fault { Fault::Capacity }
}
""", "error conversion"),
        ):
            source.write_text(variant)
            _, negative = observer(project, host)
            assert obligation in " ".join(negative["unresolved"]), negative
            assert "cases" not in negative
    print("Verified separate error orders, exact native bytes and three unresolved controls.")


if __name__ == "__main__":
    main()
