#!/usr/bin/env python3
"""Recover a bounded error contract, compile its Rust projection, and test both."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

cli = Path(sys.argv[1]).resolve()
source = '''
#[derive(Debug, thiserror::Error)]
pub enum Fault {
    #[error("device unavailable")]
    Device(#[from] std::io::Error),
    #[error("reading: {}", category(.0))]
    Reading(sample::Reading),
    #[error("measurement: {0}")]
    Measurement(&'static str),
    #[error("stop")]
    Stop,
}

pub type Outcome<T> = std::result::Result<T, Fault>;

pub(crate) fn category(code: &sample::Reading) -> &'static str {
    use sample::Reading::*;
    match code { Missing => "absent", Broken => "unreadable" }
}
'''
probe = '''
extern crate self as sample;
#[derive(Debug)]
pub enum Reading { Missing, Broken }
mod errors;
fn main() {
    use std::error::Error;
    use errors::{Fault, Outcome};
    let err: Fault = std::io::Error::other("private device message").into();
    let result: Outcome<()> = Err(err);
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), "device unavailable");
    assert_eq!(err.source().unwrap().to_string(), "private device message");
    assert!(matches!(err, Fault::Device(_)));
    assert_eq!(Fault::Measurement("42").to_string(), "measurement: 42");
    assert_eq!(Fault::Stop.to_string(), "stop");
    assert_eq!(Fault::Reading(Reading::Missing).to_string(), "reading: ABSENT");
    assert_eq!(Fault::Reading(Reading::Broken).to_string(), "reading: unreadable");
    assert!(Fault::Reading(Reading::Missing).source().is_none());
}
'''


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


with tempfile.TemporaryDirectory() as scratch:
    root = Path(scratch)
    selection, input_path = root / "selection.json", root / "error.rs"
    for name, label in (("baseline", "absent"), ("mutated", "missing")):
        changed = source.replace('"absent"', json.dumps(label))
        input_path.write_text(changed)
        selection.write_text(json.dumps(dict(
            format="oddlang-finite-error-selection-v1", id="urn:sample:faults",
            sourceSha256=hashlib.sha256(changed.encode()).hexdigest(),
            error="Fault", result="Outcome",
        )))
        recovered, output = root / name, root / (name + "-rust")
        run(cli, "recover", "finite-error-domain", input_path, selection, recovered)
        run(cli, "build", "finite-error-domain", "rust", recovered / "graph.json",
            recovered / "rust.json", output)
        (root / "errors.rs").write_bytes((output / "errors.rs").read_bytes())
        caller, program = root / "caller.rs", root / "probe"
        caller.write_text(probe.replace("ABSENT", label))
        run("rustc", "--edition=2021", caller, "-o", program)
        run(program)
    assert (root / "baseline/graph.json").read_bytes() != (root / "mutated/graph.json").read_bytes()

    def reject(fragment, *argv):
        result = subprocess.run(argv, text=True, capture_output=True)
        assert result.returncode != 0 and fragment in result.stderr, result.stderr

    output = root / "rejected"
    baseline = root / "baseline"
    graph = json.loads((baseline / "graph.json").read_bytes())
    overlay = json.loads((baseline / "rust.json").read_bytes())
    graph["variants"][next(key for key in graph["variants"] if key.endswith(":Measurement"))]["source"] = True
    invalid_graph = root / "invalid-graph.json"
    invalid_graph.write_text(json.dumps(graph))
    reject("requires an I/O error payload", cli, "build", "finite-error-domain", "rust",
           invalid_graph, baseline / "rust.json", output)
    table = next(iter(overlay["categoryTypes"]))
    overlay["categoryTypes"][table][-1] = 'Reading; panic!()'
    invalid_overlay = root / "invalid-rust.json"
    invalid_overlay.write_text(json.dumps(overlay))
    reject("invalid native category type", cli, "build", "finite-error-domain", "rust",
           baseline / "graph.json", invalid_overlay, output)
    for malformed in (source.replace('Broken => "unreadable"', '_ => "unreadable"'),
                      source.replace('match code', 'match other'),
                      source.replace('category(.0)', 'category(.1)')):
        input_path.write_text(malformed)
        spec = json.loads(selection.read_bytes())
        spec["sourceSha256"] = hashlib.sha256(malformed.encode()).hexdigest()
        selection.write_text(json.dumps(spec))
        reject("category", cli, "recover", "finite-error-domain", input_path, selection, output)
    assert not output.exists()
    caller.write_text(probe.replace("Missing, Broken", "Missing, Broken, NewCase"))
    reject("non-exhaustive patterns", "rustc", "--edition=2021", caller, "-o", program)
