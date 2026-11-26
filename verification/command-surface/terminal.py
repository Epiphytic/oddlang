"""Compare an independently derived auxiliary parser with graph-only output."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

FIXTURE = Path(__file__).resolve().parent
ODDLANG = Path(sys.argv[1]).resolve()
SOURCE = """
use clap::{CommandFactory, Parser};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(name = "telescope survey", about = "Inspect sample labels")]
struct Survey {
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u64).range(1..=9))]
    limit: u64,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1.., required = true)]
    labels: Vec<OsString>,
}

fn main() -> ExitCode {
    let args = std::env::args_os().skip(2);
    let parsed = Survey::command().try_get_matches_from(
        std::iter::once(OsString::from("telescope survey")).chain(args),
    );
    match parsed {
        Ok(_) => {
            eprintln!("command operation unavailable");
            ExitCode::from(125)
        }
        Err(error) => {
            let invalid = error.use_stderr();
            error.print().unwrap();
            if invalid {
                eprintln!("telescope: invalid arguments");
                ExitCode::from(12)
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}
"""
ENV = {**os.environ, "NO_COLOR": "1", "TERM": "dumb"}
CASES = [["--help"], ["-h"], [], ["--limit=0"], ["--limit=9", "first", "-second"], ["--unknown"]]


def run(*args, cwd=None, check=True):
    return subprocess.run(args, cwd=cwd, check=check, capture_output=True, env=ENV, timeout=90)


def observe(executable, args):
    result = run(executable, "survey", *args, check=False)
    return result.returncode, result.stdout, result.stderr


with tempfile.TemporaryDirectory(prefix="oddlang-terminal-cli-") as scratch:
    root = Path(scratch)
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copyfile(FIXTURE / name, root / name)
    bin_dir = root / "src/bin"
    bin_dir.mkdir(parents=True)
    (bin_dir / "generated.rs").write_text(
        '#[path = "../cli.rs"] mod cli;\nfn main() -> std::process::ExitCode {\n'
        '    cli::run_cli()\n}\n'
    )
    selection = root / "selection.json"
    selection.write_text(json.dumps({"root": "Survey", "package_name": "observatory", "version": ""}))
    generated = root / "graph.json"
    run(ODDLANG, "recover", "cli-surface", FIXTURE / "src/bin/reference.rs",
        FIXTURE / "selection.json", generated)
    primary = json.loads(generated.read_bytes())
    primary["root"]["subcommands"].append({"name": "survey"})
    auxiliary = root / "auxiliary.json"
    reference = bin_dir / "reference.rs"
    observations = []
    for source in (SOURCE, SOURCE.replace("1..=9", "1..=8").replace("Inspect sample labels", "Review survey labels")):
        reference.write_text(source)
        run(ODDLANG, "recover", "cli-surface", reference, selection, auxiliary)
        primary["terminal_subparsers"] = {"survey": json.loads(auxiliary.read_bytes())["root"]}
        generated.write_text(json.dumps(primary))
        run(ODDLANG, "build", "cli-surface", "rust", generated, FIXTURE / "binding.json", root / "src/cli.rs")
        run("cargo", "+1.97.1", "build", "--locked", "--offline", "--target-dir", root / "target", cwd=root)
        expected = [observe(root / "target/debug/reference", args) for args in CASES]
        actual = [observe(root / "target/debug/generated", args) for args in CASES]
        assert actual == expected, (actual, expected)
        observations.append(actual)
    assert observations[0][0][0] == observations[1][0][0] == 0
    assert observations[0][0][1] != observations[1][0][1]
    assert observations[0][4][0] == 125 and observations[1][4][0] == 12
    assert all(not payload for status, payload, _ in observations[0][2:] if status)
    assert not (observations[0][4][1] or b"invalid arguments" in observations[0][4][2])

    primary["terminal_subparsers"]["unregistered"] = primary["terminal_subparsers"]["survey"]
    generated.write_text(json.dumps(primary))
    reject = run(ODDLANG, "build", "cli-surface", "rust", generated, FIXTURE / "binding.json",
                 root / "src/cli.rs", check=False)
    assert reject.returncode and b"unknown terminal parser trigger" in reject.stderr
    del primary["terminal_subparsers"]["unregistered"]
    generated.write_text(json.dumps(primary))
    binding = root / "binding.json"
    dispatch = json.loads((FIXTURE / "binding.json").read_bytes())
    dispatch["operations"]["survey"] = "urn:observatory:survey"
    binding.write_text(json.dumps(dispatch))
    reject = run(ODDLANG, "build", "cli-surface", "rust", generated, binding, root / "src/cli.rs", check=False)
    assert reject.returncode and b"invalid terminal parser" in reject.stderr
    primary["root"]["subcommands"][-1]["aliases"] = ["inspect"]
    generated.write_text(json.dumps(primary))
    reject = run(ODDLANG, "build", "cli-surface", "rust", generated, FIXTURE / "binding.json",
                 root / "src/cli.rs", check=False)
    assert reject.returncode and b"invalid terminal parser" in reject.stderr
    reference.write_text(SOURCE.replace("num_args = 1..", "num_args = 2.."))
    reject = run(ODDLANG, "recover", "cli-surface", reference, selection, auxiliary, check=False)
    assert reject.returncode and b"one-or-more variadic argument counts" in reject.stderr
    print("Auxiliary parser matched six invocations in both fixture revisions and rejected four unsupported inputs.")
