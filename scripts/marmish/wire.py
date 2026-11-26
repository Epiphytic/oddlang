"""Exercise both directions across the original and actual reconstructed Rust crates."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from observations import CONTRACT, build_core_consumer, digest, expand, observe


def compare(upstream, candidate, output):
    cases = json.loads(CONTRACT.read_bytes())["cases"]
    report = {"cases": [], "directions": [], "publicApi": {}}
    with tempfile.TemporaryDirectory(prefix="oddlang-wire-") as stage:
        binaries = {}
        for role, workspace in [("original", upstream), ("candidate",candidate)]:
            temp = Path(stage)/role
            temp.mkdir()
            env=dict(os.environ,CARGO_NET_OFFLINE="true",RUSTUP_AUTO_INSTALL="0")
            binary, _, _, _ = build_core_consumer(workspace, temp, env)
            binaries[role]=binary
            observed = [observe(binary, case) for case in cases]
            if role=="original":
                report["cases"] = observed
            elif observed != report["cases"]:
                raise ValueError("assembled codec observations differ from upstream")
            report["publicApi"][role] = "tested"
        for writer,reader in [("original","candidate"),("candidate","original")]:
            witnesses = []
            for case in cases:
                if case["operation"]!="encode" or "bytes" not in case["expected"]:
                    continue
                raw=expand(case["input"])
                frame=subprocess.check_output([binaries[writer],"encode"],input=raw)
                decoded=subprocess.check_output([binaries[reader],"decode"],input=frame)
                if decoded != raw or frame != expand(case["expected"]["bytes"]):
                    raise ValueError(f"wire mismatch for {writer} to {reader}, {case['id']}")
                witnesses.append({"id":case["id"],"wireSha256":digest(frame),"rawSha256":digest(raw)})
            if not witnesses:
                raise ValueError("empty cross-version wire corpus")
            report["directions"].append({"writer":writer,"reader":reader,"exchanges":witnesses})
    output.write_text(json.dumps(report,sort_keys=True,indent=2)+"\n")


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("upstream", "candidate", "output"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    compare(args.upstream.resolve(), args.candidate.resolve(), args.output)
