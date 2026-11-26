"""Compare source-derived terminal option policy with an independent Clap witness."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

COMPILER = Path(sys.argv[1]).resolve()
FIXTURE = Path(__file__).resolve().parent
SOURCE = r'''
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "Primary readings")]
struct Primary {
    #[arg(short = 'p', long, global = true)] place: Option<String>,
    #[arg(short = 'a', long, global = true)] agent: Option<String>,
    #[arg(long, global = true)] brief: bool,
    #[command(subcommand)] command: Option<Mode>,
}
#[derive(Subcommand)]
enum Mode { Survey }
#[derive(Parser)]
#[command(name = "meter survey", about = "Survey readings")]
struct Survey { label: String }

fn accept(field: &str) -> bool { matches!(field, "place" | "brief") }
fn invalid(reason: &'static str) -> clap::Error {
    clap::Error::raw(ErrorKind::InvalidValue, format!("meter: {reason}\n"))
}
fn check(before: &[&str]) -> Result<(), clap::Error> {
    if before.iter().any(|field| !accept(field)) {
        return Err(invalid("option rejected"));
    }
    Ok(())
}
fn main() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let mut prefix = Vec::new();
    let mut index = 1;
    while let Some(argument) = args.get(index).and_then(|argument| argument.to_str()) {
        let (id, count) = match argument {
            "--brief" => ("brief", 1),
            "-p" | "--place" => ("place", 2),
            "-a" | "--agent" => ("agent", 2),
            value if value.starts_with("--place=") || (value.starts_with("-p") && value.len() > 2) => ("place", 1),
            value if value.starts_with("--agent=") || (value.starts_with("-a") && value.len() > 2) => ("agent", 1),
            _ => break,
        };
        prefix.push(id);
        index += count;
    }
    let root = Primary::command().bin_name("meter");
    let parsed = if args.get(index).is_some_and(|arg| arg == "survey") {
        check(&prefix).and_then(|_| Survey::command().try_get_matches_from(
            std::iter::once(OsString::from("meter survey")).chain(args[index+1..].iter().cloned())))
    } else {
        root.try_get_matches_from(args)
    };
    match parsed {
        Ok(_) => { eprintln!("command operation unavailable"); ExitCode::from(125) }
        Err(error) => {
            let invalid = error.use_stderr();
            error.print().unwrap();
            if invalid { eprintln!("meter: invalid arguments"); ExitCode::from(18) }
            else { ExitCode::SUCCESS }
        }
    }
}
'''
CASES = [
    ["--brief", "survey", "--help"], ["--place=west", "survey", "-h"],
    ["--place", "west", "survey", "--help"], ["-pwest", "survey", "--help"],
    ["-p", "west", "survey", "--help"], ["--agent=east", "survey", "--help"],
    ["-aeast", "survey", "--help"], ["-a", "east", "survey", "--help"],
    ["--brief", "survey"], ["--help", "survey"], ["--place", "survey", "--help"],
    ["--bogus", "survey", "--help"], ["help", "survey"],
]


def run(*command, cwd=None, check=True):
    result = subprocess.run(command, cwd=cwd, capture_output=True, timeout=120)
    if check and result.returncode:
        raise RuntimeError(result.stderr.decode())
    return result


def recover(instruction, source, selection, graph):
    run(COMPILER, "recover", instruction, source, selection, graph)
    return json.loads(graph.read_bytes())


with tempfile.TemporaryDirectory(prefix="odd-meter-prefix-") as temp:
    root = Path(temp)
    for file in ("Cargo.toml", "Cargo.lock"):
        shutil.copy(FIXTURE / file, root / file)
    bins = root / "src/bin"
    bins.mkdir(parents=True)
    (bins / "candidate.rs").write_text(
        '#[path="../cli.rs"] mod cli;\nfn main() -> std::process::ExitCode { cli::run_cli() }\n')
    selection = root / "selection.json"
    graph = root / "graph.json"
    source = bins / "reference.rs"
    binding = root / "rust.json"
    binding.write_text(json.dumps({"executable": "meter", "invalid_exit": 18,
                                   "invalid_diagnostic": "meter: invalid arguments",
                                   "uncovered_exit": 125, "operations": {}}))
    outputs = []
    for text in (SOURCE, SOURCE.replace('"place" | "brief"', '"place" | "brief" | "agent"')):
        source.write_text(text)
        selection.write_text(json.dumps({"root": "Primary", "package_name": "observatory", "version": "2.1.0"}))
        primary = recover("cli-surface", source, selection, graph)
        selection.write_text(json.dumps({"root": "Survey", "package_name": "observatory", "version": ""}))
        auxiliary = recover("cli-surface", source, selection, graph)
        selection.write_text(json.dumps({"allow": "accept", "route": "check", "error": "invalid",
                                         "supplied": "before", "item": "field"}))
        policy = recover("cli-terminal-prefix", source, selection, graph)
        primary["terminal_subparsers"] = {"survey": auxiliary["root"]}
        primary["terminal_prefixes"] = {"survey": policy}
        graph.write_text(json.dumps(primary))
        run(COMPILER, "build", "cli-surface", "rust", graph, binding, root / "src/cli.rs")
        run("cargo", "+1.97.1", "build", "--locked", "--offline", cwd=root)
        for argv in CASES:
            ref = run(root / "target/debug/reference", *argv, check=False)
            got = run(root / "target/debug/candidate", *argv, check=False)
            assert (ref.returncode, ref.stdout, ref.stderr) == (got.returncode, got.stdout, got.stderr), argv
        outputs.append(run(root / "target/debug/candidate", *CASES[5], check=False).returncode)
        accepted = run(root / "target/debug/candidate", "--brief", "survey", "label", check=False)
        assert accepted.returncode == 18
        assert b"unexpected argument 'label'" in accepted.stderr
        flag_like = ["-p", "--brief", "survey", "--help"]
        raw = run(root / "target/debug/reference", *flag_like, check=False)
        fallback = run(root / "target/debug/candidate", *flag_like, check=False)
        assert raw.returncode == 0 and b"Survey readings" in raw.stdout
        assert fallback.returncode == 18 and not fallback.stdout
        assert b"a value is required for '--place" in fallback.stderr
        if len(outputs) == 1:
            denied = ["--agent=east", "survey", "label"]
            ref = run(root / "target/debug/reference", *denied, check=False)
            got = run(root / "target/debug/candidate", *denied, check=False)
            assert (ref.returncode, ref.stdout, ref.stderr) == (got.returncode, got.stdout, got.stderr)
            assert b"meter: option rejected" in got.stderr
    assert outputs == [18, 0]
    primary["terminal_prefixes"]["survey"]["allowed"].append("missing-option")
    graph.write_text(json.dumps(primary))
    rejected = run(COMPILER, "build", "cli-surface", "rust", graph, binding, root / "invalid.rs", check=False)
    assert rejected.returncode and b"invalid terminal prefix policy" in rejected.stderr
    for invalid, message in (
        (SOURCE.replace('matches!(field, "place" | "brief")', 'field.starts_with("p")'),
         b"expected a literal allowlist predicate"),
        (SOURCE.replace('"place" | "brief"', '"place" | "place"'), b"duplicate allowed option identifier"),
        (SOURCE.replace("!accept(field)", "accept(field)"), b"expected one direct rejected-prefix branch"),
        (SOURCE.replace("ErrorKind::InvalidValue", "ErrorKind::UnknownArgument"), b"unsupported raw diagnostic format"),
    ):
        source.write_text(invalid)
        rejected = run(COMPILER, "recover", "cli-terminal-prefix", source, selection, graph, check=False)
        assert rejected.returncode and message in rejected.stderr
    primary["terminal_prefixes"] = {"absent": policy}
    graph.write_text(json.dumps(primary))
    rejected = run(COMPILER, "build", "cli-surface", "rust", graph, binding, root / "invalid.rs", check=False)
    assert rejected.returncode and b"invalid terminal prefix policy" in rejected.stderr
    primary["terminal_prefixes"] = {"survey": {**policy, "denied": "invalid\ntext"}}
    graph.write_text(json.dumps(primary))
    rejected = run(COMPILER, "build", "cli-surface", "rust", graph, binding, root / "invalid.rs", check=False)
    assert rejected.returncode and b"invalid terminal prefix policy" in rejected.stderr
print("13 terminal cases match in both source revisions; successful parsing stays unbound.")
