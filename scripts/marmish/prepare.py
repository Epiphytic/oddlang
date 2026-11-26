"""Materialize the mixed assembly store from immutable Git blobs and verified Oddlang inputs."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
DESTINATIONS = {"frame.rs": "crates/marmish-core/src/frame.rs"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def prepare(upstream, imported, projection, output):
    project = tomllib.loads((ROOT / "examples/marmish/project.toml").read_text())
    pin = project["git_revision"]
    entries = subprocess.check_output(["git", "-C", upstream, "ls-tree", "-r", "-z", pin])
    if output.exists() and any(output.iterdir()):
        raise ValueError("materialization requires empty output")
    output.mkdir(parents=True, exist_ok=True)
    store = output / "store"
    retained = store / "retained"
    retained.mkdir(parents=True)
    manifest = (projection / "projection.json").read_bytes()
    spec = json.loads(manifest)
    lock_hash = digest((imported / "import.lock").read_bytes())
    graph_hash = digest((imported / "domain.ttl").read_bytes())
    release = json.loads((ROOT / "examples/marmish/framing-release.json").read_bytes())
    if release != {"sourceRevision": pin, "graphSha256": graph_hash, "importSha256": lock_hash,
                   "projectionSha256": digest(manifest), "witSha256": digest((projection / "world.wit").read_bytes())}:
        raise ValueError("Marmish inputs differ from the published framing release")
    if spec["graphSha256"] != graph_hash or set(spec["files"]) != set(DESTINATIONS):
        raise ValueError("unexpected module set or graph")
    shutil.copytree(imported, store / "imports" / lock_hash)
    producer = store / "projections" / digest(manifest)
    producer.mkdir(parents=True)
    (producer / "projection.json").write_bytes(manifest)
    for name, expected in spec["files"].items():
        data = (projection / name).read_bytes()
        if digest(data) != expected:
            raise ValueError(f"projection digest mismatch for {name}")
        (producer / name).write_bytes(data)
    modules = {path: name for name, path in DESTINATIONS.items()}
    rows = []
    cargo_lock = None
    with subprocess.Popen(["git","-C",upstream,"cat-file","--batch"], stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE) as git:
        for record in entries.rstrip(b"\0").split(b"\0"):
            info, path_bytes = record.split(b"\t", 1)
            mode, kind, oid = info.decode().split()
            path = path_bytes.decode()
            if kind != "blob" or mode not in {"100644","100755"}:
                raise ValueError(f"unsupported Git mode at {path}")
            row = {"path": path, "mode": mode}
            if path in modules:
                name = modules.pop(path)
                row.update(producer="generated", module=name, sha256=spec["files"][name])
            else:
                git.stdin.write(oid.encode()+b"\n")
                git.stdin.flush()
                actual, kind, length = git.stdout.readline().decode().split()
                if actual != oid or kind != "blob":
                    raise ValueError(f"unexpected Git object at {path}")
                data = git.stdout.read(int(length))
                if len(data) != int(length) or git.stdout.read(1) != b"\n":
                    raise ValueError(f"truncated Git object at {path}")
                blob_hash = digest(data)
                row.update(producer="retained", sha256=blob_hash)
                target = retained / blob_hash
                if not target.exists():
                    target.write_bytes(data)
                if path == "Cargo.lock":
                    cargo_lock = blob_hash
            rows.append(row)
        git.stdin.close()
        if git.wait() != 0:
            raise ValueError("Git object reader failed")
    if modules or not cargo_lock:
        raise ValueError("missing generated destination or Cargo lock")
    header = {"version": 1, "crates": project["workspace_members"], "target": "aarch64-apple-darwin",
              "features": [], "graphSha256": graph_hash, "importSha256": lock_hash,
              "projectionSha256": digest(manifest), "cargoLockSha256": cargo_lock}
    def fields(items):
        return "".join(f"{key} = {json.dumps(value)}\n" for key,value in items.items())
    text = fields(header) + "".join("\n[[files]]\n"+fields(row) for row in rows)
    pinned_manifest = (ROOT / "examples/marmish/assembly.sha256").read_text().split()[0]
    if digest(text.encode()) != pinned_manifest:
        raise ValueError("complete assembly plan differs from its published digest")
    (output / "project.toml").write_text(text)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("upstream","imported","projection","output"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    prepare(args.upstream, args.imported, args.projection, args.output)
