"""Compare explicit and receipt-backed components of a source-free native package."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
CARGO = ("cargo", "+1.97.1")
RECORD_ID = "urn:weather:forecast"
MODULE_ID = "urn:weather:module"
CHOICE_ID = "urn:weather:sky"
CLEAR_ID = "urn:weather:sky:clear"


def run(*args, cwd, failure=None, env=None):
    result = subprocess.run(args, cwd=cwd, check=False, capture_output=True, text=True, env=env)
    if failure:
        assert result.returncode and failure in result.stderr, result
    else:
        assert not result.returncode, result


def dump(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
    return path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


with tempfile.TemporaryDirectory(prefix="oddlang-assemble-weather-") as scratch:
    root = Path(scratch)
    inputs = root / "inputs"
    inputs.mkdir()
    (root / "seed" / "src").mkdir(parents=True)
    library = root / "seed/src/lib.rs"
    library.write_text("mod weather_model;\npub use weather_model::*;\n")
    source = root / "seed/src/weather_model.rs"
    source.write_text("""use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sky { Clear }

#[derive(Clone, Serialize, Deserialize)]
pub struct Forecast { pub sky: Sky }
""")
    (root / "seed/Cargo.toml").write_text(
        '[package]\nname="weather"\nversion="0.1.0"\nedition="2024"\n'
        '[dependencies]\nserde={version="=1.0.229", features=["derive"]}\n'
    )
    run(*CARGO, "generate-lockfile", "--offline", cwd=root / "seed")
    lock = inputs / "Cargo.lock"
    lock.write_bytes((root / "seed/Cargo.lock").read_bytes())

    def pin(path):
        return {"path": str(path.relative_to(inputs)), "sha256": sha(path)}

    env = dict(os.environ, RUSTC_BOOTSTRAP="1", CARGO_TARGET_DIR=str(root / "target"))
    run(*CARGO, "rustdoc", "--lib", "--offline", "--locked", "--", "-Z", "unstable-options",
        "--output-format", "json", "--document-private-items", cwd=root / "seed", env=env)
    rustdoc = root / "target/doc/weather.json"
    choose = dump(root / "choice.json", {
        "format": "oddlang-rust-serde-string-choice-selection-v1", "sourceSha256": sha(source),
        "id": "urn:weather:choice-domain", "enumId": CHOICE_ID, "name": "Sky",
        "variants": {"Clear": CLEAR_ID},
    })
    select = dump(root / "record.json", {
        "format": "oddlang-serde-public-record-selection-v1", "sourceFilename": "src/weather_model.rs",
        "sourceSha256": sha(source), "crateRootSha256": sha(library), "rustdocSha256": sha(rustdoc),
        "crateName": "weather", "moduleId": MODULE_ID, "moduleName": "weather_model",
        "record": {"name": "Forecast", "id": RECORD_ID, "fields": {"sky": "urn:weather:forecast:sky"}},
        "choices": [{"name": "Sky", "id": CHOICE_ID}],
    })
    run(CLI, "recover", "serde-string-choice", source, choose, inputs / "choice", cwd=root)
    run(CLI, "recover", "serde-public-record", source, rustdoc, select, inputs / "record", cwd=root)
    shutil.rmtree(root / "seed")

    def component(folder, kind, name):
        work = inputs / folder
        return ({"kind": kind, "module": "weather_model", "reexports": [name],
                 "graph": pin(work / "graph.json"), "overlay": pin(work / "rust.json")},
                {"recovery": pin(work / "recovery.lock")})

    choice, compact_choice = component("choice", "shared-declarations", "Sky")
    record, compact_record = component("record", "public-records", "Forecast")
    anchored_choice = dict(compact_choice, module="weather_model")
    manifest = dict(format="oddlang-native-package-v1", name="weather", version="0.1.0",
                    edition="2024", dependencies={"serde": {"version": "1.0.229", "features": ["derive"]}},
                    cargoLock=pin(lock))

    def build(label, components, failure=None, **policy):
        output = root / label
        source = dump(inputs / f"{label}.json", dict(manifest, components=components) | policy)
        run(CLI, "build", "native-package", "rust", source, output, cwd=root, failure=failure)
        assert output.is_dir() != bool(failure)
        return output

    explicit = build("explicit", [choice, record])
    compact = build("compact", [anchored_choice, compact_record])
    mixed = build("mixed", [choice, compact_record])
    for lane, expected_pins in ((compact, [anchored_choice, compact_record]), (mixed, [None, compact_record])):
        for path in sorted(explicit.rglob("*")):
            if path.is_file() and path.name != "package.lock":
                assert path.read_bytes() == (lane / path.relative_to(explicit)).read_bytes()
        original = json.loads((explicit / "package.lock").read_text())
        actual = json.loads((lane / "package.lock").read_text())
        for row, expected, identity in zip(actual["components"], expected_pins, [CHOICE_ID, RECORD_ID], strict=True):
            assert row.pop("recoverySha256", None) == (expected and expected["recovery"]["sha256"])
            assert row.pop("recoveredDeclaration", None) == (expected and identity)
        assert {k: v for k,v in actual.items() if k != "manifestSha256"} == {
            k: v for k,v in original.items() if k != "manifestSha256"
        }

    build("missing-serde", [anchored_choice], "requires native serde with derive", dependencies={})
    build("missing-placement", [compact_choice, compact_record], "explicit module")
    build("placement-override", [anchored_choice, dict(compact_record, module="other")], "already determines")
    build("duplicate-provider", [anchored_choice, compact_record, compact_record], "ambiguous recovered public declaration")
    build("duplicate-export", [anchored_choice, compact_record, record], "conflicting reexport")
    build("missing-provider", [compact_record], "unresolved external public record choice")
    build("ambiguous-link", [anchored_choice, choice, compact_record], "ambiguous external public record choice")
    build("mixed-entry", [anchored_choice, {**compact_record, **record}], "package manifest")
    build("bad-receipt", [anchored_choice, {"recovery": dict(compact_record["recovery"], sha256="0" * 64)}],
          "input digest mismatch")
    append = inputs / "record/rust.json"
    append.write_bytes(append.read_bytes() + b" ")
    build("bad-sibling", [anchored_choice, compact_record], "input digest mismatch")

    run(*CARGO, "check", "--lib", "--locked", "--offline", cwd=compact, env=env)

print("Compact and explicit public-data packages emit identical native bytes")
