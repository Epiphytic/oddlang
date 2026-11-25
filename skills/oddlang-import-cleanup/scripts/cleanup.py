"""Capture, review and explicitly accept an Oddlang Rust import cleanup."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def digest(data):
    return hashlib.sha256(data).hexdigest()


def files(root):
    result = {}
    for path in sorted(root.rglob("*")):
        name = path.relative_to(root).as_posix()
        if name.split("/")[0] in {".git", "target"}:
            continue
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError(f"unsupported input {name}")
        if path.is_file():
            result[name] = digest(path.read_bytes())
    return result


def tree_digest(entries):
    return digest(b"".join(name.encode() + b"\0" + value.encode() + b"\n"
                           for name, value in sorted(entries.items())))


def freeze(args):
    if args.original.is_symlink() or not args.original.is_dir():
        raise ValueError("expected original crate directory")
    if args.pin.resolve().is_relative_to(args.original.resolve()):
        raise ValueError("write the independent baseline pin outside the input")
    entries = files(args.original)
    if not entries:
        raise ValueError("empty baseline")
    pin = f"oddlang-rust-cleanup-baseline-v1 {tree_digest(entries)}\n"
    with args.pin.open("xb") as output:
        output.write(pin.encode("ascii"))


def capture(source, dest):
    dest.mkdir()
    for name in files(source):
        path = dest / name
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / name, path)


def receipt(root, manifest, mapping):
    before, after = (files(root / tree) for tree in ["original", "effective"])
    if manifest not in before or mapping not in before or before.keys() != after.keys():
        raise ValueError("cleanup requires existing files and the pinned manifest and mapping")
    changed = [name for name in before if before[name] != after[name]]
    if not changed or any(not (name.endswith(".rs") or name == mapping) for name in changed):
        raise ValueError("expected changes to Rust source or the pinned mapping")
    return dict(version=1, manifest=manifest, mapping=mapping,
                originalTreeSha256=tree_digest(before), effectiveTreeSha256=tree_digest(after),
                patchSha256=digest((root / "cleanup.patch").read_bytes()))


def propose(args):
    if args.output.exists() or any(args.output.resolve().is_relative_to(path.resolve())
                                   for path in [args.original, args.edited]):
        raise ValueError("use a fresh output directory outside both inputs")
    with tempfile.TemporaryDirectory(dir=args.output.parent) as tmp:
        root = Path(tmp) / "proposal"
        root.mkdir()
        capture(args.original, root / "original")
        capture(args.edited, root / "effective")
        diff = subprocess.run(["git", "diff", "--no-index", "--no-ext-diff", "--no-textconv",
                               "--no-renames", "--binary", "--", "original", "effective"],
                              cwd=root, stdout=subprocess.PIPE)
        if diff.returncode != 1:
            raise ValueError("expected a nonempty diff")
        (root / "cleanup.patch").write_bytes(diff.stdout)
        proposed = receipt(root, args.manifest, args.mapping)
        data = (json.dumps(proposed, sort_keys=True, indent=2) + "\n").encode()
        (root / "proposal.json").write_bytes(data)
        root.rename(args.output)
        print(digest(data))


def accept(args):
    root = args.proposal
    data = (root / "proposal.json").read_bytes()
    if digest(data) != args.sha256:
        raise ValueError("review digest mismatch")
    record = json.loads(data)
    if receipt(root, record["manifest"], record["mapping"]) != record:
        raise ValueError("proposal changed after review")
    with (root / "accepted.json").open("xb") as output:
        output.write(data)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(__doc__)
    sub = parser.add_subparsers(required=True)
    f = sub.add_parser("freeze")
    f.add_argument("original", type=Path)
    f.add_argument("pin", type=Path)
    f.set_defaults(run=freeze)
    p = sub.add_parser("propose")
    for name in ["original", "edited", "output"]:
        p.add_argument(name, type=Path)
    p.add_argument("--manifest", default="Cargo.toml")
    p.add_argument("--mapping", default="mapping.toml")
    p.set_defaults(run=propose)
    a = sub.add_parser("accept")
    a.add_argument("proposal", type=Path)
    a.add_argument("--sha256", required=True)
    a.set_defaults(run=accept)
    args = parser.parse_args()
    try:
        args.run(args)
    except (OSError, ValueError) as err:
        parser.exit(1, f"cleanup: {err}\n")
