"""Qualify recovery of a neutral string enum with actual POSIX-host Rust."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
CARGO = ("cargo", "+1.97.1")
SOURCE = """use serde::{Deserialize, Serialize as WireSerialize};
#[derive(Debug, Clone, PartialEq, Eq, WireSerialize, Deserialize)]
pub enum Terrain {
    #[serde(rename = "wet-ground")]
    Marsh,
    Granite,
}
"""
CHANGED = SOURCE.replace('    #[serde(rename = "wet-ground")]\n    Marsh,\n    Granite,',
                         '    Granite,\n    #[serde(rename = "shallow-marsh")]\n    Marsh,')


def run(*argv, work, failure=None, env=None):
    result = subprocess.run(argv, cwd=work, env=env, check=False, capture_output=True, text=True)
    if failure:
        assert result.returncode and failure in result.stderr, result
    else:
        assert not result.returncode, result
    return result.stdout


with tempfile.TemporaryDirectory(prefix="oddlang-terrain-choice-") as tmp:
    root = Path(tmp)
    env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    selection = {
        "format": "oddlang-rust-serde-string-choice-selection-v1",
        "id": "urn:terrain:choice-domain",
        "enumId": "urn:terrain:terrain",
        "name": "Terrain",
        "variants": {"Marsh": "urn:terrain:marsh", "Granite": "urn:terrain:granite"},
    }

    def recover(source, name, failure=None, stale=False):
        work = root / name
        work.mkdir()
        path = work / "source.rs"
        path.write_text(source)
        config = {**selection, "sourceSha256": hashlib.sha256(
            (SOURCE if stale else source).encode()).hexdigest()}
        (work / "selection.json").write_text(json.dumps(config))
        run(CLI, "recover", "serde-string-choice", path, "selection.json", "recovered",
            work=work, failure=failure)
        assert (work / "recovered").exists() != bool(failure)
        return work

    def native(work, include, values, copied):
        src = work / "src"
        src.mkdir()
        variants = ", ".join(f"Terrain::{v}" for v, _ in values)
        expected = ", ".join(json.dumps(json.dumps(v)) for _, v in values)
        body = f'''
fn require<T: Clone + std::fmt::Debug + Eq + serde::Serialize + for<'de> serde::Deserialize<'de>>() {{}}
fn main() {{
    require::<Terrain>();
    let choices = [{variants}];
    let expected = [{expected}];
    for (index, (choice, wire)) in choices.into_iter().zip(expected).enumerate() {{
        assert_eq!(choice, serde_json::from_str::<Terrain>(wire).unwrap());
        assert_eq!(serde_json::to_string(&choice).unwrap(), wire);
        assert_eq!(choice.clone() as usize, index);
        println!("{{choice:?}} {{wire}}");
    }}
    for bad in ["null", "0", "{{}}", "\\"unknown\\""] {{
        assert!(serde_json::from_str::<Terrain>(bad).is_err());
    }}
}}
'''
        (src / "main.rs").write_text(include + "\n" + body)
        (work / "Cargo.toml").write_text(
            '[package]\nname="terrain-recovery"\nversion="0.0.0"\nedition="2024"\n'
            '[dependencies]\nserde={version="=1.0.229",features=["derive"]}\nserde_json="=1.0.154"\n'
        )
        run(*CARGO, "generate-lockfile", "--offline", work=work, env=env)
        output = run(*CARGO, "run", "--offline", "--locked", work=work, env=env)
        (src / "main.rs").write_text(include + "\nfn main() { fn copy<T: Copy>() {} copy::<Terrain>(); }")
        run(*CARGO, "check", "--offline", "--locked", work=work, env=env,
            failure=None if copied else "Terrain: Copy")
        return output

    for label, source, values, copied in (
        ("baseline", SOURCE, [("Marsh", "wet-ground"), ("Granite", "Granite")], False),
        ("changed", CHANGED, [("Granite", "Granite"), ("Marsh", "shallow-marsh")], False),
        ("copy", SOURCE.replace("Clone,", "Clone, Copy,"), [("Marsh", "wet-ground"), ("Granite", "Granite")], True),
    ):
        work = recover(source, label)
        repeated = recover(source, label + "-repeat")
        for file in ("graph.json", "rust.json", "recovery.lock"):
            assert (work / "recovered" / file).read_bytes() == (repeated / "recovered" / file).read_bytes()
        graph = json.loads((work / "recovered/graph.json").read_bytes())["stringEnums"][0]
        assert [v["wire"] for v in graph["variants"]] == [v for _, v in values]
        assert graph.get("copy", True) == copied
        run(CLI, "build", "shared-declarations", "rust", "recovered/graph.json", "recovered/rust.json",
            "projected", work=work)
        orig, emitted = work / "orig", work / "emitted"
        orig.mkdir()
        emitted.mkdir()
        assert native(orig, source, values, copied) == native(
            emitted, (work / "projected/shared.rs").read_text(), values, copied,
        )

    recover(CHANGED, "stale", "source digest mismatch", stale=True)
    recover(SOURCE.replace("serde::{", "other::{"), "wrong-import", "unresolved Serde derive")
    recover(SOURCE.replace("use serde", "use fault::Serialize as WireSerialize;\nuse serde"),
            "ambiguous-import", "ambiguous derive binding")
    recover(SOURCE.replace('rename = "wet-ground"', 'rename(serialize = "wet-ground")'),
            "directional", "unsupported Serde variant attribute")
    recover(SOURCE.replace("    Granite,", "    Granite(u64),"), "payload", "expected unconditional unit variant")
    recover(SOURCE.replace("    Granite,", "    Granite = 2,"), "discriminant", "expected unconditional unit variant")
    recover(SOURCE.replace("pub enum Terrain", '#[serde(rename_all = "snake_case")]\npub enum Terrain'),
            "rename-all", "unsupported enum attribute")
    recover(SOURCE + "\nimpl Terrain { pub fn extra(&self) {} }", "extra-impl", "selected enum has an additional binding")
    recover(SOURCE + "\nuse local::*;", "glob", "unsupported derive scope")

print("Pinned Serde string choices, native capabilities, spelling/order mutation and rejection controls pass.")
