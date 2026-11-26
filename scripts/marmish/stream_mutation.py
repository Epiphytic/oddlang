"""Reauthorize one graph leaf and verify that the independent wire oracle catches it."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

from check_build_closure import checked_run, profile, require, save, sha
from qualify_stream_workspace import (
    ORACLE, boundaries, clean_environment, denied_reads, digest, git_archive, regular_inputs, tool_paths,
)

MODULE = "src/copy_protocol.rs"
GRAPH_LITERAL = b'<urn:oddlang:rust-tree:file:src/copy_protocol.rs#7816> <urn:oddlang:rust-tree:spelling>'
SOURCE_CONTEXT = b"payload.extend(bytes);\n                5\n"
DIFFERENCES = {"binary-data-encode", "maximum-data-encode"}


def rewrite(bundle, verified_source):
    graph = bundle / "syntax.ttl"
    statement = GRAPH_LITERAL + b' "5" .\n'
    raw = graph.read_bytes()
    require(raw.count(statement) == 1, "expected one frozen Data tag graph leaf")
    graph.write_bytes(raw.replace(statement, GRAPH_LITERAL + b' "12" .\n'))

    inventory = bundle / "sources.json"
    record = json.loads(inventory.read_text())
    source = verified_source.read_bytes()
    require(digest(verified_source) == record["files"][MODULE] and source.count(SOURCE_CONTEXT) == 1,
            "expected the verified emitted copy protocol source")
    record["files"][MODULE] = sha(source.replace(SOURCE_CONTEXT, SOURCE_CONTEXT[:-2] + b"12\n"))
    inventory.write_text(json.dumps(record, indent=2) + "\n")
    lock = bundle / "bundle.lock"
    value = json.loads(lock.read_text())
    value.update(syntaxSha256=digest(graph), sourcesSha256=digest(inventory))
    lock.write_text(json.dumps(value, indent=2) + "\n")
    return digest(lock)


def main(args):
    output = args.output.resolve()
    output.mkdir()
    root = output / "mutated"
    archive_sha = git_archive(args.repository, args.commit, root)
    package = root / "crates/marmish-stream"
    require(not any(name.endswith(".rs") for name in regular_inputs(package)),
            "expected a source-free Git archive")
    bundle = root / "oddlang/stream"
    base_pin = digest(bundle / "bundle.lock")
    require((root / "oddlang/stream.sha256").read_text().strip() == base_pin,
            "base bundle differs from the committed release pin")
    pin = rewrite(bundle, args.candidate / "crates/marmish-stream" / MODULE)
    (root / "oddlang/stream.sha256").write_text(pin + "\n")

    tools = tool_paths()
    toolchain = tools[0]
    trees, files = boundaries(*tools)
    writable = output / "writable"
    env = clean_environment(writable, root, *tools)
    emit = output / "emit.sb"
    emit.write_text(profile(trees, writable, [*files, args.oddlang, *bundle.iterdir()]))
    source_probe = args.candidate / "crates/marmish-stream" / MODULE
    probes = denied_reads(emit, {"unmodifiedCandidate": source_probe}, writable, env)
    emitted = writable / "generated"
    command = [args.oddlang, "build", "rust-tree", bundle, emitted, "--bundle-sha256", pin]
    checked_run(command, "emit", writable, env, output, emit)
    (emitted / "src").rename(package / "src")

    config = Path(env["CARGO_HOME"]) / "config.toml"
    config.write_bytes(args.vendor_config.read_bytes())
    build = output / "build.sb"
    build.write_text(profile([*trees, root, args.vendor], writable, files))
    probes.update(denied_reads(build, {"unmodifiedCandidateDuringBuild": source_probe}, root, env))
    receipt = output / "oracle-mutation.json"
    command = [sys.executable, "-B", ORACLE / "run.py", "--workspace", root, "--target-dir",
               writable / "oracle-target", "--receipt", receipt, "--role", "mutation",
               "--baseline", args.baseline, "--commit", args.commit, "--archive-sha256", archive_sha,
               "--toolchain", toolchain, "--sandbox", build]
    status = subprocess.run(command, env=env, cwd=root).returncode
    require(status != 0 and receipt.exists(), "mutation did not build and reach the independent oracle")
    observations = json.loads(receipt.read_text())
    mismatches = {row["name"] for row in observations["observations"] if not row["matched"]}
    require(mismatches == DIFFERENCES, f"unexpected mutation case spread: {sorted(mismatches)}")
    save(output / "report.json", {"baseBundleSha256": base_pin, "bundleSha256": pin, "oracleSha256": digest(receipt),
                                 "changedLeaf": GRAPH_LITERAL.decode(), "deniedReads": probes,
                                 "observedDifferences": sorted(mismatches)})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("repository", "oddlang", "candidate", "baseline", "vendor", "vendor-config", "output"):
        parser.add_argument("--" + name, required=True, type=lambda value: Path(value).resolve())
    parser.add_argument("--commit", required=True)
    main(parser.parse_args())
