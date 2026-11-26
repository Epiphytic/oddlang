"""Compare advertised equality capabilities against original and generated Rust types."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def rust(source, library, root):
    return subprocess.run(["rustc", "+1.97.1", "--edition=2021", "--extern", f"sampling={library}",
                           "--error-format=json", "-", "-o", root / "caller"], input=source,
                          text=True, capture_output=True)


def verify(cli, root, mode, derives):
    source = root / "sampling.rs"
    source.write_text(f"#[derive({derives})] pub enum Sampling {{ Nearest, Linear }}")
    selected = root / "selection.json"
    selected.write_text(json.dumps({"format": "oddlang-plain-choice-selection-v1", "id": "urn:sampling",
                                    "name": "Sampling", "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest()}))
    bundle = root / f"bundle-{mode}"
    subprocess.run([cli, "recover", "plain-choice", source, selected, bundle], check=True)
    entry, = json.loads((bundle / "graph.json").read_bytes())["types"].values()
    assert entry.get("equality") == (None if mode == "total" else mode)
    generated = root / f"generated-{mode}"
    subprocess.run([cli, "build", "checked-values", "rust", bundle / "graph.json",
                    bundle / "rust.json", generated], check=True)
    obligation, = json.loads((generated / "obligations.json").read_bytes())["obligations"]
    expected_capability = {"none": "none", "partial": "partial-case-identity", "total": "case-identity"}
    assert obligation["expected"]["equality"] == expected_capability[mode]
    library = root / "libsampling.rlib"
    for path in (source, generated / "values.rs"):
        subprocess.run(["rustc", "+1.97.1", "--crate-name", "sampling", "--crate-type", "rlib", path,
                        "-o", library], check=True)
        for trait, expected in (("Copy", True), ("Clone", True), ("PartialEq", mode != "none"), ("Eq", mode == "total")):
            case = rust(f"fn require<T: {trait}>() {{}} fn main() {{ require::<sampling::Sampling>(); }}", library, root)
            if expected:
                assert case.returncode == 0, case.stderr
            else:
                codes = [d["code"]["code"] for line in case.stderr.splitlines()
                         if (d := json.loads(line)).get("code")]
                assert case.returncode != 0 and codes == ["E0277"], case.stderr
        if mode != "none":
            case = rust("use sampling::Sampling::*; fn main() { assert!(Nearest == Nearest && Nearest != Linear); }",
                        library, root)
            assert case.returncode == 0, case.stderr
            subprocess.run([root / "caller"], check=True)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as work:
        for mode, traits in (("none", "Clone, Copy"), ("partial", "PartialEq, Copy, Clone"),
                             ("total", "Eq, Copy, PartialEq, Clone")):
            verify(Path(sys.argv[1]).resolve(), Path(work), mode, traits)
    print("Three original and generated choice equality contracts passed.")
