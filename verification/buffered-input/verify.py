"""Compile a real Rust shape fixture and observe the projected private buffer."""
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

FRAMEWORK = Path(__file__).resolve().parents[2]
CLI = Path(sys.argv[1]).resolve()
ENV = dict(os.environ, RUSTUP_TOOLCHAIN="1.97.1", CARGO_NET_OFFLINE="true")
SOURCE = """
struct Lookahead<T> {
    feed: T,
    octets: zeroize::Zeroizing<[u8; 7]>,
    used: usize,
    loaded: usize,
}
"""
PROBE = r"""
#[cfg(test)]
mod tests {
    use super::Lookahead;
    use std::io::{self, BufRead, Read};

    #[test]
    fn observable_transitions() {
        struct Samples(u8);
        impl Read for Samples {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => { out[..3].copy_from_slice(&[17, 41, 83]); Ok(3) }
                    2 => Err(io::ErrorKind::BrokenPipe.into()),
                    _ => Ok(0),
                }
            }
        }

        let mut reader = Lookahead {
            feed: Samples(0), octets: zeroize::Zeroizing::new([0; 7]), used: 0, loaded: 0,
        };
        fn needs_erasing_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}
        needs_erasing_drop(&reader.octets);
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        assert_eq!(reader.feed.0, EMPTY_CALLS);
        assert_eq!(reader.fill_buf().unwrap(), &[17, 41, 83]);
        assert_eq!(reader.feed.0, 1);
        let mut first = [0; 2];
        assert_eq!(reader.read(&mut first).unwrap(), 2);
        assert_eq!(first, [17, 41]);
        assert_eq!(&reader.octets[..3], &[AFTER_TWO, 83]);
        reader.consume(usize::MAX);
        assert_eq!(reader.used, 3);
        assert_eq!(&reader.octets[..3], &[AFTER_ALL]);
        assert_eq!(reader.fill_buf().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        assert_eq!((reader.used, reader.loaded), (0, OLD_END));
        if OLD_END != 0 {
            assert_eq!(reader.fill_buf().unwrap(), &[AFTER_ALL]);
            assert_eq!(reader.feed.0, 2);
            reader.consume(3);
        }
        assert!(reader.fill_buf().unwrap().is_empty());
        assert_eq!((reader.used, reader.loaded, reader.feed.0), (0, 0, 3));
        assert_eq!(reader.read(&mut first).unwrap(), 0);
    }
}
"""


def run(*args, cwd, error=None, env=ENV):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=240)
    stderr = result.stderr.decode(errors="replace")
    if error is None:
        assert not result.returncode, stderr
    else:
        assert result.returncode and error in stderr, (args, stderr)
    return result.stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")
    return path


with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    source = root / "src"
    source.mkdir()
    manifest_path = root / "Cargo.toml"
    manifest_path.write_text('[package]\nname="light-meter"\nversion="0.1.0"\n'
                             'edition="2021"\n[dependencies]\nzeroize="=1.9.1"\n')
    lib = source / "lib.rs"
    lib.write_text(SOURCE)
    run("cargo", "generate-lockfile", "--offline", cwd=root)
    host = next(line[6:] for line in run("rustc", "-vV", cwd=root).decode().splitlines()
                if line.startswith("host: "))

    def observe(name):
        raw = run("bash", FRAMEWORK / "verification/rust-recovery/run.sh", manifest_path,
                  "light-meter", "lib", "light_meter", host, "default", cwd=root,
                  env=dict(ENV, ODD_RECOVERY_DATA_SHAPES="1"))
        assert b'"compiler"' in raw
        path = root / name
        path.write_bytes(raw)
        return path

    original = observe("original.json")
    changed_source = SOURCE.replace("    feed: T,\n    octets: zeroize::Zeroizing<[u8; 7]>,",
                                    "    octets: zeroize::Zeroizing<[u8; 11]>,\n    feed: T,")
    lib.write_text(changed_source)
    reordered = observe("reordered.json")
    lib.unlink()

    selected = {
        "id": "urn:test:meter:lookahead",
        "declaration": "local::light_meter::Lookahead::{type}",
        "fields": dict(input="feed", storage="octets", begin="used", end="loaded"),
        "transitions": dict(emptyRead="refill-if-exhausted", refillError="reset-begin-retain-end",
                            consumedBytes="erase", releaseStorage="erase"),
    }
    selector = write(root / "selection.json", selected)
    for dataset, name in ((original, "baseline"), (reordered, "mutation")):
        out = root / name
        run(CLI, "recover", "buffered-input", dataset, selector, out, cwd=root)
        repeat = root / (name + "-repeat")
        run(CLI, "recover", "buffered-input", dataset, selector, repeat, cwd=root)
        assert sorted(p.name for p in out.iterdir()) == ["graph.json", "recovery.lock", "rust.json"]
        assert all(p.read_bytes() == (repeat / p.name).read_bytes() for p in out.iterdir())
        graph = json.loads((out / "graph.json").read_bytes())
        assert graph["capacity"] == (7 if dataset == original else 11)
        expected = ["input", "storage"] if dataset == original else ["storage", "input"]
        assert graph["fieldOrder"] == expected + ["begin", "end"]
        receipt = json.loads((out / "recovery.lock").read_bytes())
        assert receipt["observationSha256"] == digest(dataset)
        assert receipt["selectionSha256"] == digest(selector)
    baseline = root / "baseline"
    args = (CLI, "recover", "buffered-input", original, selector, baseline)
    initial_files = {p.name: digest(p) for p in baseline.iterdir()}
    run(*args, cwd=root, error="publish fresh package")
    assert initial_files == {p.name: digest(p) for p in baseline.iterdir()}

    raw = json.loads(original.read_bytes())
    entry = next(d for d in raw["graph"]["declarations"] if d["id"] == selected["declaration"])
    storage = next(f for f in entry["fields"] if f["name"] == "octets")
    storage["type"]["id"] = "local::light_meter::Zeroizing::{type}"
    invalid = write(root / "shadow.json", raw)
    run(CLI, "recover", "buffered-input", invalid, selector, root / "invalid",
        cwd=root, error="expected fixed erasing octet storage")
    assert not (root / "invalid").exists()
    raw = json.loads(original.read_bytes())
    raw["graph"]["gaps"].append({"owner": selected["declaration"] + "::used",
                                  "reason": "fixture-incomplete"})
    run(CLI, "recover", "buffered-input", write(root / "gap.json", raw), selector,
        root / "gap", cwd=root, error="unresolved selected buffer shape")
    assert not (root / "gap").exists()

    overlay = json.loads((baseline / "rust.json").read_bytes())
    original_graph = json.loads((baseline / "graph.json").read_bytes())
    manifest = {
        "format": "oddlang-native-package-v1", "name": "light-meter", "version": "0.1.0",
        "edition": "2021", "dependencies": {"zeroize": {"version": "1.9.1"}},
        "cargoLock": {"path": "Cargo.lock", "sha256": digest(root / "Cargo.lock")},
    }

    def assemble(name, graph, native, *, error=None, **extra):
        graph_path = write(root / "graph.json", graph)
        overlay_path = write(root / "rust.json", native)
        manifest["components"] = [{
            "kind": "buffered-input", "module": "meter", "reexports": [],
            "graph": {"path": graph_path.name, "sha256": digest(graph_path)},
            "overlay": {"path": overlay_path.name, "sha256": digest(overlay_path)},
        } | extra]
        package = write(root / "package.json", manifest)
        out = root / name
        run(CLI, "build", "native-package", "rust", package, out, cwd=root, error=error)
        assert out.exists() != bool(error)
        return out

    assemble("public", original_graph, overlay, visibility="public",
             error="buffered-input must stay private")
    assemble("reexport", original_graph, overlay, reexports=["Lookahead"],
             error="buffered-input must stay private")
    assemble("zero", original_graph | {"capacity": 0}, overlay, error="nonempty fixed buffer")
    assemble("negative", original_graph | {"capacity": -1}, overlay, error="invalid value")
    assemble("too-big", original_graph | {"capacity": 2**63}, overlay, error="layout bound")
    assemble("duplicate-role", original_graph | {"fieldOrder": ["input"] * 4}, overlay,
             error="each buffered-input field role")
    manifest["dependencies"] = {}
    assemble("missing-provider", original_graph, overlay, error="locked registry provider zeroize")
    manifest["dependencies"] = {"zeroize": {"version": "1.9.1"}}
    cargo_lock = root / "Cargo.lock"
    original_lock = cargo_lock.read_bytes()
    cargo_lock.write_bytes(original_lock.replace(b'e1308439', b'00000000'))
    manifest["cargoLock"]["sha256"] = digest(cargo_lock)
    assemble("altered-provider", original_graph, overlay, error="locked registry source and checksum")
    cargo_lock.write_bytes(original_lock)
    manifest["cargoLock"]["sha256"] = digest(cargo_lock)
    assemble("shadow-parameter", original_graph, overlay | {"parameter": "usize"},
             error="invalid buffered-input Rust binding")

    stable = [assemble(name, original_graph, overlay) for name in ("stable-a", "stable-b")]
    for relative in ("package.lock", "src/group_0/__projection_0.rs"):
        assert (stable[0] / relative).read_bytes() == (stable[1] / relative).read_bytes()

    for i, (empty, stale, erase) in enumerate(itertools.product((False, True), repeat=3)):
        graph = original_graph | {"transitions": original_graph["transitions"] | {
            "emptyRead": "return-zero" if empty else "refill-if-exhausted",
            "refillError": "reset-begin-retain-end" if stale else "discard-window",
            "consumedBytes": "erase" if erase else "retain",
        }}
        product = assemble(f"policy-{i}", graph, overlay)
        lock = json.loads((product / "package.lock").read_bytes())
        leaf = "src/group_0/__projection_0.rs"
        assert lock["generatedFiles"][leaf] == digest(product / leaf)
        assert "pub " not in (product / leaf).read_text()
        probe = PROBE.replace("EMPTY_CALLS", str(int(not empty))).replace("OLD_END", str(3 if stale else 0))
        probe = probe.replace("AFTER_TWO", "0, 0" if erase else "17, 41")
        probe = probe.replace("AFTER_ALL", "0, 0, 0" if erase else "17, 41, 83")
        with (product / leaf).open("a") as code:
            code.write(probe)
        output = run("cargo", "test", "--locked", "--offline", "--quiet",
                     "--target-dir", root / "tested", cwd=product)
        assert b"1 passed" in output
    print("Real Rust array/order mutation, fail-closed controls and eight native buffered-input policies passed.")
