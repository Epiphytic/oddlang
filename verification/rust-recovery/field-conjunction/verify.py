"""Check generic compiler-backed field conjunctions against native Rust."""
import argparse
import hashlib
import json
import os

from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
METHOD = "crate::Invoice::agrees"
STAGE = "#[derive(PartialEq)]\npub enum Stage { Draft, Sent }\n"
BASE = """
mod types;
use types::Stage;

pub struct Invoice { pub serial: u32, pub flags: Vec<u8>, pub stage: Stage }
pub struct Receipt { pub serial: u32, pub backup: u32, pub flags: Vec<u8>, pub stage: Stage }

impl Invoice {
    pub fn agrees(&self, other: &Receipt) -> bool {
        other.serial == self.serial && self.flags == other.flags && self.stage == other.stage
    }
}
"""
PRODUCT = BASE[:BASE.index("impl Invoice")]
CALLER = """
fn main() {
    let value = Invoice { serial: 3, flags: vec![17], stage: Stage::Sent };
    for (serial, backup, flags, stage) in [
        (3, 0, 17, Stage::Sent), (0, 3, 17, Stage::Sent),
        (3, 3, 18, Stage::Sent), (3, 3, 17, Stage::Draft),
    ] {
        let other = Receipt { serial, backup, flags: vec![flags], stage };
        println!("{}", value.agrees(&other));
    }
}
"""
CUSTOM_STAGE = STAGE.replace("#[derive(PartialEq)]\n", "") + """
impl PartialEq for Stage {
    fn eq(&self, _: &Self) -> bool { false }
}
"""


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs)


def write(path, obj):
    path.write_text(json.dumps(obj, sort_keys=True) + "\n")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def shapes(temp):
    ident = "urn:sample:"
    relation, receiver, argument = (ident + x for x in ("agrees", "invoice", "receipt"))
    values = dict(serial={"kind": "unsigned", "bits": 32}, flags={"kind": "bytes"},
                  stage={"kind": "named", "id": ident + "stage"})
    types, bindings = {ident + "stage": {"fields": {}, "equality": True}}, {}
    for record in (receiver, argument):
        fields = values if record == receiver else values | {"backup": values["serial"]}
        types[record] = {"fields": {record + ":" + key: val for key, val in fields.items()}}
        bindings.update({record + ":" + key: key for key in fields})
    write(temp / "template.json", dict(
        format="oddlang-field-relation-v1", id=ident + "graph", types=types, calls={},
        relations={relation: dict(receiver=receiver,
            arguments=[dict(id=ident+"other", value=dict(kind="named", id=argument))],
            steps=[], failure=None)}))
    write(temp / "rust.json", dict(
        format="oddlang-field-relation-rust-v1", types={
            receiver: "crate::Invoice", argument: "crate::Receipt", ident+"stage": "crate::types::Stage"},
        fields=bindings, calls={}, relations={relation: dict(name="agrees", arguments={ident+"other":"other"})},
        errorType=None, errors={}))
    return relation


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("oddlang", type=Path)
    args = parser.parse_args()
    cli = args.oddlang.resolve()
    host = next(line.split()[1] for line in run("rustc", "+1.97.1", "-vV").splitlines()
                if line.startswith("host:"))
    with tempfile.TemporaryDirectory() as directory:
        temp = Path(directory)
        project = temp / "fixture"
        (project / "src").mkdir(parents=True)
        manifest = project / "Cargo.toml"
        source = project / "src/lib.rs"
        stage = project / "src/types.rs"
        stage.write_text(STAGE)
        (temp / "types.rs").write_text(STAGE)
        manifest.write_text('[package]\nname="fixture"\nversion="0.1.0"\nedition="2024"\n')
        source.write_text(BASE)
        run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=project)
        relation = shapes(temp)
        evidence, selection = (temp / x for x in ("evidence.json", "selection.json"))
        cases = {
            "base": (BASE, None, "true\nfalse\nfalse\nfalse\n"),
            "mutated": (BASE.replace("other.serial", "other.backup"), None,
                        "false\ntrue\nfalse\nfalse\n"),
            "custom": (BASE, "not proven effect-free", None),
            "effect": (BASE.replace("other.serial ==", 'eprintln!("effect"); other.serial =='),
                       "statements, aliases or effects", None),
            "alias": (BASE.replace("other.serial ==", "let alias = other; alias.serial =="),
                      "statements, aliases or effects", None),
            "shadow": (BASE.replace("other.serial ==", "let other = self; other.serial ==")
                       ,
                       "statements, aliases or effects", None),
        }
        for name, (text, reason, expected) in cases.items():
            source.write_bytes(text.replace("\n", "\r\n").encode() if name == "base" else text.encode())
            stage.write_text(CUSTOM_STAGE if name == "custom" else STAGE)
            with evidence.open("w") as stream:
                subprocess.run(["bash", HERE.parent/"run.sh", manifest, "fixture", "lib",
                                "fixture", host, "default"], env=dict(os.environ,
                                ODD_RECOVERY_FIELD_CONJUNCTION=METHOD), stdout=stream, check=True)
            write(selection, dict(format="oddlang-field-conjunction-selection-v1", relation=relation,
                                  sourceSha256=sha(source), evidenceSha256=sha(evidence)))
            graph = temp / f"{name}.json"
            command = [cli, "recover", "field-conjunction", project, source, evidence, selection,
                       temp/"template.json", temp/"rust.json", graph]
            if reason:
                result = subprocess.run(command, capture_output=True, text=True)
                assert result.returncode and reason in result.stderr and not graph.exists(), result.stderr
                continue
            run(*command)
            if name == "base":
                stage.write_text(CUSTOM_STAGE)
                stale = temp / "stale.json"
                result = subprocess.run([*command[:-1], stale], capture_output=True, text=True)
                assert result.returncode and "inventory changed: src/types.rs" in result.stderr
                assert not stale.exists()
                stage.write_text(STAGE)
            repeat = temp / "repeat.json"
            repeat.unlink(missing_ok=True)
            run(*command[:-1], repeat)
            assert graph.read_bytes() == repeat.read_bytes()
            generated = temp / name
            run(cli, "build", "field-relation", "rust", graph, temp/"rust.json", generated)
            projection, = generated.glob("*.rs")
            for label, implementation in (("original", text), ("projected", PRODUCT+projection.read_text())):
                case = temp / f"{label}.rs"
                case.write_text(implementation + CALLER)
                exe = temp / "case"
                run("rustc", "+1.97.1", "--edition=2024", case, "-o", exe)
                assert run(exe) == expected, (name, label)
        base = json.loads((temp/"base.json").read_bytes())["relations"][relation]["steps"]
        mutant = json.loads((temp/"mutated.json").read_bytes())["relations"][relation]["steps"]
        assert base[1:] == mutant[1:]
        assert base[0]["left"]["field"].endswith(":serial")
        assert mutant[0]["left"]["field"].endswith(":backup")
    print("Qualified unrelated native equality, field mutation, cross-file invalidation and four unresolved controls.")


if __name__ == "__main__":
    main()
