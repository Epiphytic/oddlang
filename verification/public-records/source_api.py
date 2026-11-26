"""Qualify checked Rust drop-in API requirements with an unrelated source caller."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
RECORD = "urn:calendar:appointment"
FIELD = "urn:calendar:appointment:until"
GRAPH = {"format": "oddlang-public-records-v1", "modules": [{
    "id": "urn:calendar:appointments", "aliases": [],
    "records": [{"id": RECORD, "traits": ["debug", "serialize", "deserialize"],
                 "fields": [{"id": FIELD, "wire": "until", "type": {"optional": "u64"}}]}],
}]}
OVERLAY = {"format": "oddlang-public-records-rust-v1", "flattenModule": True,
           "identifiers": {"urn:calendar:appointments": "calendar", RECORD: "Appointment"}}
REQUIREMENT = {"name": "until", "visibility": "public", "type": {"optional": "u64"}}
SOURCE_API = {"role": "rust-drop-in-source-api", "fields": {FIELD: REQUIREMENT}}

CALLER = r'''
use calendar_api::Appointment;

fn main() {
    for input in [r#"{}"#, r#"{"until":null}"#] {
        let open = serde_json::from_str::<Appointment>(input).unwrap();
        let field: Option<u64> = open.until;
        assert_eq!(field, None);
        assert_eq!(serde_json::to_string(&open).unwrap(), r#"{"until":null}"#);
    }
    for value in [0, 9007199254740993_u64, u64::MAX] {
        let text = format!(r#"{{"until":{value}}}"#);
        let appointment = Appointment { until: Some(value) };
        let field: Option<u64> = appointment.until;
        assert_eq!(serde_json::to_string(&appointment).unwrap(), text);
        assert_eq!(serde_json::from_str::<Appointment>(&text).unwrap().until, field);
    }
    for bad in [r#"{"until":18446744073709551616}"#, r#"{"until":-1}"#, r#"{"until":"1"}"#] {
        assert!(serde_json::from_str::<Appointment>(bad).is_err());
    }
}
'''


def run(*args, work, error=None):
    result = subprocess.run(args, cwd=work, text=True, capture_output=True, check=False)
    if error:
        assert result.returncode and error in result.stderr, result
    else:
        assert not result.returncode, result


with tempfile.TemporaryDirectory(prefix="oddlang-calendar-source-api-") as scratch:
    root = Path(scratch)
    graph = root / "graph.json"
    overlay = root / "rust.json"
    graph.write_text(json.dumps(GRAPH))

    def project(label, source_api=None, error=None):
        binding = dict(OVERLAY)
        if source_api is not None:
            binding["sourceApi"] = source_api
        overlay.write_text(json.dumps(binding))
        output = root / label
        run(CLI, "build", "public-records", "rust", graph, overlay, output, work=root, error=error)
        assert output.exists() != bool(error)
        return output

    old = project("legacy")
    checked = project("checked", SOURCE_API)
    assert (old / "records.rs").read_bytes() == (checked / "records.rs").read_bytes()
    for label, change, error in (
        ("field-name", {"name": "ending"}, "source API field name differs"),
        ("field-type", {"type": {"optional": "u32"}}, "source API field type differs"),
        ("field-visibility", {"visibility": "private"}, "unknown variant"),
    ):
        project(label, dict(SOURCE_API, fields={FIELD: dict(REQUIREMENT, **change)}), error)
    project("wrong-role", dict(SOURCE_API, role="wire"), "unknown variant")
    project("unknown-field", dict(SOURCE_API, fields={RECORD: REQUIREMENT}), "unknown source API field")
    project("empty", dict(SOURCE_API, fields={}), "source API field requirements are empty")
    binding = json.dumps(dict(OVERLAY, sourceApi=SOURCE_API))
    overlay.write_text(binding.replace(json.dumps(FIELD) + ":", json.dumps(FIELD) + ": "
                                      + json.dumps(REQUIREMENT) + ", " + json.dumps(FIELD) + ":", 1))
    run(CLI, "build", "public-records", "rust", graph, overlay, root / "duplicate",
        work=root, error="duplicate binding key")
    assert not (root / "duplicate").exists()

    (root / "src").mkdir()
    (root / "src/lib.rs").write_bytes((checked / "records.rs").read_bytes())
    (root / "src/main.rs").write_text(CALLER)
    (root / "Cargo.toml").write_text(
        '[package]\nname="calendar_api"\nversion="0.0.0"\nedition="2024"\n'
        '[dependencies]\nserde={version="=1.0.229",features=["derive"]}\nserde_json="=1.0.154"\n'
    )
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", work=root)
    run("cargo", "+1.97.1", "run", "--offline", "--locked", work=root)

print("Checked source API caller, portable optional-u64 bounds and seven rejection controls")
