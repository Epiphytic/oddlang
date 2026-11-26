"""Compare independently encoded graph bytes with an unrelated native writer."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

CLI = Path(sys.argv[1]).resolve()
SOURCE = Path(__file__).with_name("telemetry.rs").read_text()


def recover(root, text, valid=True, override=None):
    root.mkdir()
    source = root / "input.rs"
    source.write_text(text)
    selected = dict(sourceSha256=hashlib.sha256(source.read_bytes()).hexdigest(), record="Reading",
                    writer="encode", owner="urn:radio:reading", tagTypes={"Band": "urn:radio:band"})
    selected.update(override or {})
    selection = root / "selection.json"
    selection.write_text(json.dumps(selected))
    output = root / "output"
    result = subprocess.run([CLI, "recover", "record-writer", source, selection, output], capture_output=True)
    assert (result.returncode == 0) == valid, result.stderr
    assert output.exists() == valid
    if valid:
        graph = json.loads((output / "graph.json").read_bytes())
        observation = json.loads((output / "observation.json").read_bytes())
        assert observation["graphSha256"] == hashlib.sha256((output / "graph.json").read_bytes()).hexdigest()
        return graph


def compare(root, graph):
    exe = root / "original"
    subprocess.run(["rustc", "--edition=2021", root / "input.rs", "-o", exe], check=True)
    observed = subprocess.check_output([exe], text=True).splitlines()
    for tag, line in zip((3, 9), observed, strict=True):
        values = {"sequence": 257, "station": bytes([9, 0]), "band": tag,
                  "caption": bytes([0, 255, 65]), "powered": True}
        result = bytearray()
        for slot in graph["fields"]:
            name = slot["field"]
            value = values[name.removeprefix("urn:radio:reading:field:")]
            kind = slot["encoding"]
            if kind == "unsigned-be":
                result.extend(value.to_bytes(graph["fieldTypes"][name]["bits"] // 8, "big"))
            elif kind == "u32-be-length-prefixed-bytes":
                result.extend(len(value).to_bytes(4, "big") + value)
            elif kind == "fixed-bytes":
                assert len(value) == graph["fieldTypes"][name]["bytes"]
                result.extend(value)
            elif kind in ("bool-u8", "external-tag-u8"):
                result.append(value)
            else:
                raise ValueError(kind)
        assert result.hex() == line


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    graph = recover(root / "base", SOURCE)
    compare(root / "base", graph)
    assert [s["encoding"] for s in graph["fields"]] == [
        "unsigned-be", "fixed-bytes", "external-tag-u8", "u32-be-length-prefixed-bytes", "bool-u8"]
    assert recover(root / "renamed", SOURCE.replace("packet", "blob").replace("target", "out")) == graph
    a, b = "counter(sink, row.sequence);", "sink.extend(row.station);"
    mutant = recover(root / "swap", SOURCE.replace(a, "TEMP").replace(b, a).replace("TEMP", b))
    compare(root / "swap", mutant)
    assert mutant["fields"][:2] == graph["fields"][:2][::-1]
    raw = recover(root / "raw", SOURCE.replace("powered", "r#type"))
    assert raw["fields"][-1]["field"] == "urn:radio:reading:field:type"
    assert graph["externalTags"]["urn:radio:band"] == {"urn:radio:band:Low": 3, "urn:radio:band:High": 9}
    for i, (old, new) in enumerate([
        ("enum Band", "macro_rules! shape { () => { type u64 = u32; } } shape!(); enum Band"),
        ("enum Band", "#[another::inject] enum Band"),
        ("enum Band", "use another::rewrite as serde; #[serde] enum Band"),
        ("enum Band", "type r#u64 = u32; enum Band"),
        ("enum Band", "use another::Bytes as r#Vec; enum Band"),
        ("High = 9", "High = 300"),
        ("to_be_bytes", "to_le_bytes"), ("target.extend(data);", "target.extend(data); target.push(0);"),
        (a, "if row.powered { " + a + " }"), ("fn packet(", "#[cfg(unix)]\nfn packet("),
        ("struct Reading", "use custom::Vec;\nstruct Reading"), (b, b + " unknown(sink);"),
    ]):
        recover(root / str(i), SOURCE.replace(old, new), False)
    recover(root / "pin", SOURCE, False, {"sourceSha256": "0" * 64})
print("Source-derived byte order matched an independent native writer, including the reordered writer.")
