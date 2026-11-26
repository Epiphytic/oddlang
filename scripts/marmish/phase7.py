"""Qualify fresh native and component observations for the reconstructed workspace."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import gzip
import json
import os
from pathlib import Path
import subprocess
import sys

from gaps import generate
from observations import CONTRACT
from prepare import ROOT, digest
from rebuild import rebuild
from receipt import expected_cases

HERE = Path(__file__).resolve().parent


def coverage(inventory, generated):
    gaps = generate(inventory)
    retained = [m for m in gaps["modules"] if m["path"] not in generated]
    # Prioritize dependencies of first-party packages. Calls inside these packages
    # are still lexical; they do not establish a resolved function ordering.
    crates = inventory["cargo"]["crates"]
    dependencies = {
        name: {dep["name"] for dep in crate["dependencyDeclarations"]
               if dep["name"] in crates and dep["kind"] != "dev"}
        for name, crate in crates.items()
    }
    order = []
    while dependencies:
        ready = sorted(name for name,deps in dependencies.items() if not deps)
        if not ready:
            raise ValueError("cycle among first-party package dependencies")
        order.extend(ready)
        for name in ready:
            del dependencies[name]
        for deps in dependencies.values():
            deps.difference_update(ready)
    retained.sort(key=lambda m:(order.index(m["path"].split("/")[1]),m["path"]))
    sites = [s for s in inventory["symbols"] if s["classification"] == "retained-native"]
    imported = sum(s["path"] in generated for s in sites)
    if sum(m["lexicalSites"] for m in retained) + imported != len(sites):
        raise ValueError("residual register does not balance the original lexical denominator")
    return {"unit":"declaration sites in the frozen upstream runtime-source inventory",
            "regeneratedSites":imported,"retainedSites":len(sites)-imported,"knownSites":len(sites),
            "compiledRuntimeDenominator":None, "packageDependencyOrder":order,
            "retainedModules":retained,
            "blockingCapabilities":[t for t in gaps["tasks"] if any(m["blockedOn"]==t["id"] for m in retained)]}


def summarize(directory):
    def read(name):
        return json.loads((directory / name).read_bytes())
    reconstruction = read("rebuild/rebuild.json")
    native = read("native/qualification.json")
    wire = read("wire.json")
    frame = read("frame/observations/report.json")
    if (native["candidateCommit"]!=reconstruction["candidateCommit"]
        or native["originalCommit"]!=reconstruction["originalCommit"]
        or reconstruction["independentBuilds"]!=2):
        raise ValueError("native observations refer to a different build")
    generated = reconstruction["generated"]
    base = directory/"rebuild/first"
    for module, profile in [("frame.rs","framing"),("lib.rs","facade")]:
        emitted = base/f"{profile}-projection"/module
        if digest(emitted.read_bytes())!=generated[f"crates/marmish-core/src/{module}"]["sha256"]:
            raise ValueError("published module differs from the assembled bytes")
    frame_dir = directory/"frame"
    if (frame["graphSha256"]!=digest((base/"framing/domain.ttl").read_bytes())
        or (frame_dir/"projection/frame.rs").read_bytes()!=(base/"framing-projection/frame.rs").read_bytes()):
        raise ValueError("observed component differs from qualified workspace framing")
    for name, result in frame["builds"].items():
        for path, expected in result["artifacts"].items():
            if digest((frame_dir/"observations"/name/path).read_bytes())!=expected:
                raise ValueError(f"stale {name} framing observation: {path}")
    contract=json.loads(CONTRACT.read_bytes())
    cases=contract["cases"]
    if (frame["cases"]!=len(cases) or wire["cases"]!=expected_cases(contract) or wire["publicApi"]!={"original":"tested","candidate":"tested"}
        or len(wire["directions"])!=2 or any(not row["exchanges"] for row in wire["directions"])):
        raise ValueError("incomplete native/component interoperability observations")
    inventory_path=directory/"native/original/inventory/inventory.json.gz"
    with gzip.open(inventory_path) as handle:
        residuals=coverage(json.load(handle),generated)
    (directory/"coverage.json").write_text(json.dumps(residuals,indent=2,sort_keys=True)+"\n")
    summary = {"scope":"native Apple Silicon source-build comparison and framing component conformance",
               "originalCommit":native["originalCommit"],"candidateCommit":native["candidateCommit"],
               "candidateTree":reconstruction["candidateTree"],"independentBuilds":2,
               "sourceFiles":reconstruction["sourceFiles"],"retainedFilesBeforeIndexUpdate":reconstruction["retainedFiles"],
               "regeneratedRust":sorted(generated),"regeneratedIndex":reconstruction["regeneratedOntology"],
               "lexicalSites":{k:residuals[k] for k in ("knownSites","retainedSites","regeneratedSites")},
               "compiledRuntimeDenominator":None,"platforms":native["platforms"],"nativeVerdict":native["status"],
               "newRegressions":native["regressions"],"frameCases":len(cases),
               "mixedWireExchanges":sum(len(r["exchanges"]) for r in wire["directions"]),
               "inputs":{name:digest((directory/name).read_bytes()) for name in [
                   "coverage.json","wire.json","frame/observations/report.json","native/qualification.json","rebuild/rebuild.json"]},
               "wholeApplicationRegenerated":False,"universalBehaviorEquivalence":"open"}
    (directory/"qualification.json").write_text(json.dumps(summary,indent=2,sort_keys=True)+"\n")
    return summary


def verify(upstream, directory):
    directory.mkdir(parents=True,exist_ok=True)
    if any(directory.iterdir()):
        raise ValueError("verify requires an empty destination")
    rebuild(upstream, directory/"rebuild")
    candidate = directory/"rebuild/candidate"
    commands = [
        [sys.executable,"-B",HERE/"qualify_candidate.py",upstream,candidate,directory/"native"],
        [HERE/"offline.sh",sys.executable,"-B",HERE/"wire.py",upstream,candidate,directory/"wire.json"],
        [ROOT/"scripts/qualify-marmish.sh","frame",upstream,directory/"frame"]]
    with ThreadPoolExecutor(max_workers=len(commands)) as pool:
        jobs = [pool.submit(subprocess.run, cmd, cwd=ROOT) for cmd in commands]
        statuses = [job.result().returncode for job in jobs]
    if any(statuses[1:]) or statuses[0] not in (0,3):
        raise ValueError(f"incomplete qualification commands: {statuses}")
    result=summarize(directory)
    if result["newRegressions"]:
        raise ValueError("native candidate regression; inspect qualification.json")
    print(f"Qualified {result['candidateCommit']} in {directory}")


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream",type=Path)
    parser.add_argument("output",type=Path)
    args=parser.parse_args()
    verify(args.upstream.resolve(),args.output.resolve())
