"""Exercise a sealed passive record and linked enum with a separately pinned Rust role."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
FIELD = "urn:parking:stay:departure"


def dump(path, value):
    path.write_text(json.dumps(value))
    return path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


with tempfile.TemporaryDirectory(prefix="oddlang-parking-api-") as temp:
    root = Path(temp)

    def pin(path):
        return {"path": path.relative_to(root).as_posix(), "sha256": sha(path)}

    def run(*args, failure=None):
        proc = subprocess.run(args, cwd=root, text=True, capture_output=True)
        assert (proc.returncode != 0 and failure in proc.stderr) if failure else proc.returncode == 0, proc

    def input_bundle(folder, profile, graph, overlay):
        bundle = root / folder
        bundle.mkdir()
        graph_path = dump(bundle / "graph.json", graph)
        overlay_path = dump(bundle / "rust.json", overlay)
        receipt = dump(bundle / "recovery.lock", {
            "format": profile, "graphSha256": sha(graph_path), "overlaySha256": sha(overlay_path),
            "sourceSha256": "a" * 64, "selectionSha256": "b" * 64,
            "crateRootSha256": "c" * 64, "rustdocSha256": "d" * 64,
        })
        return {"recovery": pin(receipt)}, receipt, overlay_path

    stay = "urn:parking:stay"
    zone = "urn:parking:zone"
    zone_north = "urn:parking:north"
    placement = "urn:parking:records"
    choice, _, _ = input_bundle("zone", "oddlang-rust-serde-string-choice-recovery-v1",
        {"format": "oddlang-shared-declarations-v1", "id": "urn:parking:zones",
         "stringEnums": [{"id": zone, "variants": [{"id": zone_north, "wire": "North"}]}]},
        {"format": "oddlang-shared-rust-v1", "names": {zone: "Zone", zone_north: "North"},
         "variantOrder": {zone: [zone_north]}})
    choice["module"] = "parking"
    record_graph = {"format": "oddlang-public-records-v1", "modules": [{
        "id": placement, "aliases": [], "records": [{
            "id": stay, "traits": ["serialize", "deserialize"],
            "readPolicy": {"shape": "object-or-ordered-sequence", "unknownFields": "ignore"},
            "fields": [{"id": FIELD, "wire": "departure", "type": {"optional": "u64"}},
                       {"id": "urn:parking:stay:zone", "wire": "zone", "type": {"external": zone}}],
        }]
    }]}
    binding = {"format": "oddlang-public-records-rust-v1", "flattenModule": True,
               "identifiers": {placement: "parking", stay: "Stay"},
               "externalTypes": {zone: ["crate", "Zone"]}}
    record, receipt, immutable = input_bundle("stay", "oddlang-serde-public-record-recovery-lock-v1",
                                              record_graph, binding)
    role = {"role": "rust-drop-in-source-api",
            "fields": {FIELD: {"name": "departure", "visibility": "public", "type": {"optional": "u64"}}}}
    role_pin = pin(dump(root / "role.json", dict(binding, sourceApi=role)))
    (root / "src").mkdir()
    lib = root / "src/lib.rs"
    lib.touch()
    (root / "Cargo.toml").write_text(
        '[package]\nname="parking_api"\nversion="0.1.0"\nedition="2024"\n'
        '[dependencies]\nserde={version="=1.0.229",features=["derive"]}\n'
    )
    run("cargo", "+1.97.1", "generate-lockfile", "--offline")
    manifest = {"format": "oddlang-native-package-v1", "name": "parking_api", "version": "0.1.0",
                "edition": "2024", "cargoLock": pin(root / "Cargo.lock"),
                "dependencies": {"serde": {"version": "1.0.229", "features": ["derive"]}}}
    lib.unlink()

    def build(label, components, failure=None):
        out = root / label
        config = dump(root / (label + ".json"), dict(manifest, components=components))
        run(CLI, "build", "native-package", "rust", config, out, failure=failure)
        assert out.exists() != bool(failure)
        return out

    old = build("baseline", [choice, record])
    new = build("with-role", [choice, dict(record, sourceApi=role_pin)])
    assert (new / "src/group_0/__projection_1.rs").read_bytes() == (
        old / "src/group_0/__projection_1.rs").read_bytes()
    stored = json.loads((new / "package.lock").read_bytes())["components"][1]
    assert stored["overlaySha256"] == sha(immutable)
    assert stored["recoverySha256"] == sha(receipt)
    assert stored["sourceApiSha256"] == role_pin["sha256"]
    assert stored["projection"]["sourceApiSha256"] == role_pin["sha256"]
    (new / "tests").mkdir()
    (new / "tests/caller.rs").write_text(
        'use parking_api::{Stay, Zone};\n'
        '#[test] fn source_shape() { let s = Stay { departure: Some(u64::MAX), zone: Zone::North };'
        ' let end: Option<u64> = s.departure; assert_eq!(end, Some(u64::MAX)); }'
    )
    run("cargo", "+1.97.1", "test", "--offline", "--locked", "--manifest-path", new / "Cargo.toml")
    for label, bad, failure in (
        ("wrong-role", dict(binding, sourceApi=dict(role, role="wire")), "unknown variant"),
        ("wrong-name", dict(binding, sourceApi=dict(role, fields={
            FIELD: dict(role["fields"][FIELD], name="wrong")})), "source API field name differs"),
        ("drift", dict(binding, sourceApi=role, identifiers={placement: "parking", stay: "Different"}),
         "differs from recovered binding"),
        ("missing-role", binding, "requires sourceApi"),
    ):
        override = pin(dump(root / (label + "-role.json"), bad))
        build(label, [choice, dict(record, sourceApi=override)], failure)
    build("stale", [choice, dict(record, sourceApi=dict(role_pin, sha256="0" * 64))], "input digest mismatch")
    build("choice-role", [dict(choice, sourceApi=role_pin), record], "only valid for recovered public records")
    embedded, _, _ = input_bundle("embedded", "oddlang-serde-public-record-recovery-lock-v1",
                                  record_graph, dict(binding, sourceApi=role))
    build("embedded-and-separate", [choice, dict(embedded, sourceApi=role_pin)], "already has a source API")
print("Pinned separate API binding and immutable recovery compose with unchanged native output")
