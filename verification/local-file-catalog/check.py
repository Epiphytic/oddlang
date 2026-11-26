"""Test two inert naming families and an unrelated native typed catalog."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from assembly import verify

CLI = Path(sys.argv[1]).resolve()
HERE = Path(__file__).resolve().parent
GRAPH = {
    "format": "oddlang-local-file-catalog-v1", "id": "urn:test:catalog:specimens",
    "manifestGrammar": "urn:test:catalog:line-grammar", "filePolicy": "urn:test:catalog:descriptor",
    "fixedCatalog": "catalog.txt", "prefixes": ["v1-report.", "v2-report."],
    "candidateNameEncoding": "utf8", "attributeChecks": "fixed-owner-and-mode",
    "suffix": ".idx", "namingAnchor": "record", "assetNames": ["record", "extra"], "maximumCatalogBytes": 256,
    "digest": "sha256",
    "guards": ["unique-name", "bounded-read", "catalog-digest", "parse",
               "canonical-name", "ordered-assets"],
    "error": {"id": "urn:test:invalid", "ambiguous": "catalog_path", "digest": "checksum"},
}
BINDING = {
    "format": "oddlang-local-file-catalog-rust-v1", "entry": "inspect_catalog",
    "namingType": "Naming", "fixedVariant": "Fixed", "selectedVariant": "Selected",
    "names": "ITEM_NAMES",
    "record": {
        "module": ["crate", "formats"], "type": "Catalog", "parse": "parse",
        "assetName": "label", "assets": "items",
        "name": "name", "bytes": "length", "mode": "mode", "digest": "digest",
    },
    "file": {"module": ["crate", "reads"], "read": "bounded", "verify": "verify"},
    "error": ["crate", "Fault"], "invalidVariant": "Invalid", "ioVariant": "Io",
}


def run(*cmd, cwd):
    subprocess.run(cmd, cwd=cwd, check=True)


with tempfile.TemporaryDirectory() as path:
    root = Path(path)
    for name, data in (("graph.json", GRAPH), ("rust.json", BINDING)):
        (root / name).write_text(json.dumps(data))
    out = root / "generated"
    run(CLI, "build", "local-file-catalog", "rust", root / "graph.json", root / "rust.json", out, cwd=root)
    (root / "Cargo.toml").write_text(
        '[package]\nname="catalog-observer"\nversion="0.1.0"\nedition="2021"\n'
        '[dependencies]\nsha2="=0.10.9"\n')
    (root / "src").mkdir()
    (root / "src/lib.rs").write_bytes((HERE / "caller.rs").read_bytes())
    (root / "src/catalog.rs").write_bytes((out / "catalog.rs").read_bytes())
    run("cargo", "+1.97.1", "generate-lockfile", "--offline", cwd=root)
    run("cargo", "+1.97.1", "test", "--offline", "--locked", cwd=root)
    for field, value in [
        ("fixedCatalog", "../outside"),
        ("suffix", "/metadata"),
        ("prefixes", ["v", "v1"]),
        ("assetNames", ["extra", "../record"]),
        ("guards", ["parse", "catalog-digest"]),
        ("candidateNameEncoding", "raw-bytes"),
        ("attributeChecks", "both"),
    ]:
        changed = dict(GRAPH, **{field: value})
        (root / "graph.json").write_text(json.dumps(changed))
        failure = subprocess.run(
            [CLI, "build", "local-file-catalog", "rust", root / "graph.json",
             root / "rust.json", root / "refused"], capture_output=True, text=True)
        assert failure.returncode and "unsupported local file catalog policy" in failure.stderr
        assert not (root / "refused").exists()

verify(CLI, GRAPH, BINDING)
