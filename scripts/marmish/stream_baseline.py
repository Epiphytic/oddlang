"""Capture and compare stable Rust build inputs for the marmish-stream experiment."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys


ORIGINAL = "49fc705408a230bd0ad896248d08dbdd2faad05a"
RUST = "1.97.1"
RUST_COMMIT = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
PACKAGE = "marmish-stream"
PACKAGE_ROOT = "crates/marmish-stream/"
LOGICAL_ROOT = "/oddlang-workspace"
TEST_MODULE = re.compile(rb"(?m)^#\[cfg\(test\)\]\r?\nmod tests \{\r?\n")
UNINDENTED_CLOSE = re.compile(rb"(?m)^}[^\S\r\n]*$")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")


def command_output(args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def normalize(value, roots):
    for root, replacement in roots:
        if value == str(root):
            return replacement
        if value.startswith(f"{root}/"):
            return replacement + value[len(str(root)):]
    return value


def dep_info(contents):
    # These files are compiler dep-info, with a Make prerequisite list followed
    # by phony rules and rustc's environment-dependency comments.
    lines = contents.replace("\\\n", "").splitlines()
    _, inputs = lines[0].split(": ", 1)
    paths = [
        re.sub(r"\\(.)", r"\1", word)
        for word in re.findall(r"(?:\\.|[^\s\\])+", inputs)
    ]
    environment = {}
    for line in lines[1:]:
        if line.startswith("# env-dep:"):
            key, found, value = line.removeprefix("# env-dep:").partition("=")
            require(key not in environment, "duplicate rustc environment dependency " + key)
            environment[key] = value if found else None
    return paths, environment


def inline_test_span(data, path):
    # The pinned files use a formatted, top-level test module. Its first
    # column-zero closing brace ends the fixture span before any later items.
    # Whole-file hashes also pin sources independent of these scoped spans.
    matches = list(TEST_MODULE.finditer(data))
    if not matches:
        return None
    require(len(matches) == 1, f"multiple fixture test modules in {path}")
    match = matches[0]
    close = UNINDENTED_CLOSE.search(data, match.end())
    require(close is not None, f"no closing brace for the fixture test module in {path}")
    begin = match.start()
    end = close.start() + 1
    return {"startByte": begin, "endByte": end, "sha256": sha(data[begin:end])}


def tracked_sources(root, declared):
    paths = {path for path in declared if path.startswith(PACKAGE_ROOT) and path.endswith(".rs")}
    require(paths, "no tracked package Rust files")
    actual = {path.relative_to(root).as_posix() for path in (root / PACKAGE_ROOT).rglob("*.rs")}
    require(actual == paths, f"package Rust source set differs: missing {sorted(paths - actual)}, extra {sorted(actual - paths)}")
    sources = {}
    test_modules = {}
    for path in sorted(paths):
        source = root / path
        require(source.is_file() and not source.is_symlink() and source.resolve() == source,
                f"missing or invalid Rust source {path}")
        data = source.read_bytes()
        blob = b"blob " + str(len(data)).encode() + b"\0" + data
        algorithm = {40: "sha1", 64: "sha256"}[len(declared[path])]
        sources[path] = {"sha256": sha(data), "bytes": len(data),
                         "gitBlob": hashlib.new(algorithm, blob).hexdigest()}
        span = inline_test_span(data, path)
        if span is not None:
            test_modules[path] = span
    return sources, test_modules


def git_commit(root):
    result = subprocess.run(["git", "rev-parse", "--show-toplevel", "HEAD"], cwd=root,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    if result.returncode == 0:
        directory, commit = result.stdout.splitlines()
        if Path(directory).resolve() == root:
            return commit
    return None


def collect_tracked(root):
    entries = subprocess.check_output(["git", "ls-tree", "-rz", "HEAD"], cwd=root)
    result = {}
    for item in entries.split(b"\0")[:-1]:
        metadata, path = item.split(b"\t", 1)
        _, kind, blob = metadata.decode().split()
        if kind == "blob":
            result[os.fsdecode(path)] = blob
    return result


def run(command, root, env, output, stem, sandbox):
    prefix = ["/usr/bin/sandbox-exec", "-f", str(sandbox)] if sandbox else []
    result = subprocess.run([*prefix, *map(str, command)], cwd=root, env=env, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    output.joinpath(stem + ".stdout").write_bytes(result.stdout)
    output.joinpath(stem + ".stderr").write_bytes(result.stderr)
    require(result.returncode == 0, f"{stem} exited {result.returncode}; see {output / (stem + '.stderr')}")
    return result.stdout.decode(), result.stderr.decode()


def cargo_words(stderr):
    # Cargo's shell display may contain literal newlines in package metadata.
    pending = None
    for line in stderr.splitlines():
        start = re.match(r"\s+Running `(.*)", line)
        if pending is None and start:
            pending = start[1]
        elif pending is not None:
            pending += "\n" + line
        if pending is not None and pending.endswith("`"):
            try:
                words = shlex.split(pending[:-1])
            except ValueError:
                continue
            yield words
            pending = None
    require(pending is None, "incomplete verbose Cargo command")


def cargo_environment(words):
    environment = {}
    for word in words:
        key, sep, value = word.partition("=")
        if not sep or not re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", key):
            break
        environment[key] = value
    return environment, words[len(environment):]


def build_producers(messages, stderr, root, target, cargo_home, vendor, toolchain, sdk, declared, env):
    scripts = [row for row in messages if row["reason"] == "build-script-executed"]
    workspaces = [(root, LOGICAL_ROOT), (target, "$TARGET"), (cargo_home, "$CARGO_HOME"),
                  (toolchain, "$TOOLCHAIN"), (sdk, "$SDK")]
    if vendor is not None:
        workspaces.append((vendor, "$VENDOR"))
    producers = {}
    script_runs = {}
    run_output_hashes = {}
    for script in scripts:
        directory = Path(script["out_dir"])
        require(directory.is_relative_to(target), f"build script OUT_DIR escaped target: {directory}")
        identity = script["package_id"].replace(f"path+{root.as_uri()}/", "workspace:")
        require(directory not in producers, f"duplicate build script OUT_DIR: {directory}")
        producers[directory] = identity

    for words in cargo_words(stderr):
        supplied, command = cargo_environment(words)
        directory = Path(supplied.get("OUT_DIR", ""))
        if directory in producers and len(command) == 1 and str(target) in command[0]:
            require(directory not in script_runs, f"duplicate build script run: {directory}")
            script_runs[directory] = (supplied, Path(command[0]))
    require(script_runs.keys() == producers.keys(), "some Cargo build-script executions lack verbose invocations")

    facts = []
    for script in scripts:
        directory = Path(script["out_dir"])
        identity = producers[directory]
        supplied, executable = script_runs[directory]
        manifest = Path(supplied["CARGO_MANIFEST_DIR"]) / "Cargo.toml"

        def observe(path):
            return observed_path(path, root, target, cargo_home, vendor, toolchain, sdk, declared, producers)

        emitted = (directory.parent / "output").read_bytes()
        run_output_hashes[identity] = {"stdout": sha(emitted), "executable": sha(executable.read_bytes())}
        rerun_files = []
        rerun_env = {}
        directives = []
        for line in emitted.decode().splitlines():
            match = re.fullmatch(r"cargo::?([^=]+)=(.*)", line)
            if match is None:
                continue
            key, value = match.groups()
            if key == "rerun-if-changed":
                changed = (manifest.parent / value).resolve()
                if changed.is_dir():
                    rerun_files.extend(observe(path) for path in sorted(changed.rglob("*")) if path.is_file())
                elif changed.is_file():
                    rerun_files.append(observe(changed))
                else:
                    rerun_files.append(["missing-declaration", value, None])
            elif key == "rerun-if-env-changed":
                env_value = supplied.get(value, env.get(value))
                rerun_env[value] = normalize(env_value, workspaces) if env_value is not None else None
            else:
                directives.append([key, normalize(value, workspaces)])
        facts.append({
            "packageId": identity,
            "manifest": observe(manifest),
            "declaredRerunFiles": sorted(rerun_files),
            "declaredRerunEnvironment": rerun_env,
            "directives": sorted(directives),
            "declaredCfg": sorted(script["cfgs"]),
            "linkedLibraries": sorted(script["linked_libs"]),
            "coverage": "Cargo-declared rerun inputs and outputs",
        })
    return producers, sorted(facts, key=lambda row: row["packageId"]), run_output_hashes


def flag_values(args, flag):
    return [args[i + 1] for i, value in enumerate(args[:-1]) if value == flag]


def match_depfile(args):
    crate = flag_values(args, "--crate-name")[0]
    out_dir = Path(flag_values(args, "--out-dir")[0])
    extra = next((value.removeprefix("extra-filename=") for value in flag_values(args, "-C")
                  if value.startswith("extra-filename=")), "")
    return out_dir / (crate + extra + ".d")


def observed_path(raw, root, target, cargo_home, vendor, toolchain, sdk, declared, producers):
    path = (root / raw).resolve()
    roots = [(root, "workspace"), (target, "build"), (cargo_home / "registry/src", "registry"),
             (cargo_home / "git/checkouts", "git-dependency"), (toolchain, "toolchain"), (sdk, "sdk")]
    if vendor is not None:
        roots.insert(0, (vendor, "registry"))
    for ancestor, label in roots:
        if not path.is_relative_to(ancestor):
            continue
        relative = path.relative_to(ancestor).as_posix()
        if label == "workspace":
            require(relative in declared, f"undeclared compiler input {relative}")
        if label == "registry" and ancestor != vendor:
            relative = relative.split("/", 1)[1]
        if label == "build":
            producer = next((item for item in producers if path.is_relative_to(item)), None)
            require(producer is not None, f"no Cargo build script produced {path}")
            relative = producers[producer] + "/" + path.relative_to(producer).as_posix()
        require(path.is_file(), f"missing compiler input {path}")
        return [label, relative, sha(path.read_bytes())]
    raise ValueError(f"compiler dep-info escaped declared roots: {path}")


def compiler_inventory(stderr, root, target, cargo_home, vendor, toolchain, sdk, declared, producers):
    rustc = str(toolchain / "bin/rustc")
    rustdoc = str(toolchain / "bin/rustdoc")
    roots = [(root, LOGICAL_ROOT), (target, "$TARGET"), (cargo_home, "$CARGO_HOME"),
             (toolchain, "$TOOLCHAIN"), (sdk, "$SDK")]
    records = []
    documents = []
    depfiles = set()
    for words in cargo_words(stderr):
        compiler = next((word for word in words if word in (rustc, rustdoc)), None)
        if compiler is None:
            continue
        args = words[words.index(compiler) + 1:]
        if compiler == rustdoc:
            documents.append({
                "root": next(arg for arg in args if arg.endswith(".rs")),
                "scope": "rustdoc invocation; stable rustc dep-info is unavailable",
            })
            continue
        depfile = match_depfile(args)
        require(depfile.exists(), f"rustc invocation has no matching dep-info: {depfile}")
        require(depfile not in depfiles, f"repeated dep-info destination: {depfile}")
        depfiles.add(depfile)
        inputs, env_dependencies = dep_info(depfile.read_text())
        # Record the compiler-selected conditions and deps. Artifact filenames
        # can differ under relocation, so compare the selected aliases.
        environment = {
            key: None if value is None else normalize(value, roots)
            for key, value in env_dependencies.items()
        }
        source = next((arg for arg in args if arg in inputs), None)
        require(source, f"rustc invocation root did not appear in {depfile}")
        records.append({
            "crate": flag_values(args, "--crate-name")[0],
            "root": observed_path(source, root, target, cargo_home, vendor, toolchain, sdk, declared, producers),
            "test": "--test" in args,
            "sourcedCfg": sorted(flag_values(args, "--cfg")),
            "crateTypes": flag_values(args, "--crate-type"),
            "externAliases": sorted(value.split("=", 1)[0] for value in flag_values(args, "--extern")),
            "inputs": sorted(observed_path(raw, root, target, cargo_home, vendor, toolchain, sdk, declared, producers)
                             for raw in inputs),
            "environment": environment,
        })
    require(records, "no stable rustc compiler invocations observed")
    disk_depfiles = {
        path for path in target.rglob("*.d")
        if "out" not in path.relative_to(target).parts and
        (path.parent.name == "deps" or "build" in path.relative_to(target).parts)
    }
    require(disk_depfiles == depfiles,
            f"unpaired rustc dep-info: {sorted(str(path) for path in disk_depfiles ^ depfiles)}")
    return sorted(records, key=lambda row: json.dumps(row, sort_keys=True)), documents


def list_tests(stdout):
    return sorted(set(re.findall(r"(?m)^(.+): test$", stdout)))


def named_results(stdout):
    return dict(re.findall(r"(?m)^test (.+) \.\.\. (ok|FAILED|ignored)(?:,.*)?$", stdout))


def execute_suite(root, env, executable, output, stem, sandbox):
    result = {"executable": str(executable), "sha256": sha(executable.read_bytes())}
    checked = [str(executable)]
    names, _ = run([*checked, "--list"], root, env, output, stem + "-list", sandbox)
    ignored, _ = run([*checked, "--list", "--ignored"], root, env, output, stem + "-ignored", sandbox)
    require(result["sha256"] == sha(executable.read_bytes()), f"executable changed: {executable}")
    outcomes, _ = run([*checked, "--test-threads=1"], root, env, output, stem + "-results", sandbox)
    result.update({"listed": list_tests(names), "ignored": list_tests(ignored), "outcomes": named_results(outcomes)})
    require(set(result["listed"]) == result["outcomes"].keys(), f"incomplete named test outcomes: {stem}")
    require(result["ignored"] == sorted(k for k, v in result["outcomes"].items() if v == "ignored"),
            f"ignored test membership changed: {stem}")
    return result


def compare(original, candidate):
    require(original["role"] == "original" and candidate["role"] == "candidate", "receipt roles differ")
    require(original["workspaceRoot"] != candidate["workspaceRoot"], "candidate is the original workspace")
    for field in ("rustc", "cargo", "target", "targetCfg", "sourceRemapping", "cargoInputs",
                  "trackedSources", "inlineTestSpans"):
        require(original[field] == candidate[field], f"baseline mismatch: {field}")
    require(original["builds"].keys() == candidate["builds"].keys(), "baseline build modes differ")
    for mode, left in original["builds"].items():
        right = candidate["builds"][mode]
        for field in ("rustcInputs", "rustdoc", "buildScriptFacts", "artifactProfiles", "docTestNames", "docTestOutcomes"):
            require(left[field] == right[field], f"baseline mismatch: {mode}.{field}")
        require(left["suites"].keys() == right["suites"].keys(), f"test targets differ: {mode}")
        for suite, original_suite in left["suites"].items():
            actual = right["suites"][suite]
            require(all(original_suite[key] == actual[key] for key in ("listed", "ignored", "outcomes")),
                    f"named test results differ: {mode}.{suite}")


def capture(root, output, original, cargo_home, sandbox, vendor=None):
    workspace_commit = git_commit(root)
    if original is None:
        require(workspace_commit == ORIGINAL, "wrong upstream commit")
        require(not command_output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=root),
                "baseline requires a clean upstream checkout")
        declared = collect_tracked(root)
    else:
        require(original["version"] == 2 and original["upstreamCommit"] == ORIGINAL
                and original["role"] == "original", "invalid baseline")
        require(original["workspaceRoot"] != str(root), "candidate is the original workspace")
        declared = original["declaredWorkspacePaths"]
    sources, test_modules = tracked_sources(root, declared)
    cargo = Path(command_output(["rustup", "which", "--toolchain", RUST, "cargo"])).resolve()
    toolchain = cargo.parents[1]
    rustc = toolchain / "bin/rustc"
    rustdoc = toolchain / "bin/rustdoc"
    compiler = command_output([rustc, "-vV"])
    require(f"commit-hash: {RUST_COMMIT}" in compiler and "host: aarch64-apple-darwin" in compiler,
            "expected pinned native Apple Silicon rustc")
    sdk = Path(command_output(["xcrun", "--show-sdk-path"])).resolve()
    clang = Path(command_output(["xcrun", "--find", "clang"])).resolve()
    rustflags = f"--remap-path-prefix={root}={LOGICAL_ROOT}"
    home = output / "home"
    temp = output / "tmp"
    for directory in (home, temp):
        directory.mkdir()
    env = {
        "PATH": f"{toolchain}/bin:/usr/bin:/bin",
        "HOME": str(home),
        "CARGO_HOME": str(cargo_home),
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never",
        "RUSTC": str(rustc),
        "RUSTDOC": str(rustdoc),
        "CARGO_ENCODED_RUSTFLAGS": rustflags,
        "CARGO_ENCODED_RUSTDOCFLAGS": rustflags,
        "TMPDIR": str(temp),
        "SDKROOT": str(sdk),
        "CC": str(clang),
        "CXX": str(clang.parent / "clang++"),
        "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": str(clang),
        "LC_ALL": "C",
        "TZ": "UTC",
    }
    report = {
        "version": 2,
        "role": "candidate" if original is not None else "original",
        "upstreamCommit": ORIGINAL,
        "workspaceRoot": str(root),
        "workspaceGitCommit": workspace_commit,
        "rustc": compiler,
        "cargo": command_output([cargo, "-V"]),
        "target": "aarch64-apple-darwin",
        "targetCfg": sorted(command_output([rustc, "--print", "cfg"]).splitlines()),
        "sourceRemapping": LOGICAL_ROOT,
        "cargoInputs": {path: sha((root / path).read_bytes()) for path in (
            "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", PACKAGE_ROOT + "Cargo.toml"
        )},
        "declaredWorkspacePaths": declared,
        "trackedSources": sources,
        "inlineTestSpans": test_modules,
        "builds": {},
    }
    prefix = ["--locked", "--offline", "-p", PACKAGE, "--message-format=json", "-vv"]
    for mode, action in (
        ("test", ["test"]),
        ("check", ["check", "--all-targets"]),
        ("release", ["build", "--release"]),
    ):
        target = output / f"{mode}-target"
        run_env = dict(env, CARGO_TARGET_DIR=str(target))
        command = [cargo, *action, *prefix]
        stdout, stderr = run(command, root, run_env, output, mode, sandbox)
        messages = [json.loads(line) for line in stdout.splitlines() if line.startswith('{"reason":')]
        require(any(j["reason"] == "build-finished" and j["success"] for j in messages),
                f"no successful Cargo receipt for {mode}")
        artifacts = [j for j in messages if j["reason"] == "compiler-artifact"
                     and Path(j["manifest_path"]) == root / PACKAGE_ROOT / "Cargo.toml"]
        require(artifacts, f"no compiler artifacts for {PACKAGE} in {mode}")
        producers, build_facts, run_hashes = build_producers(
            messages, stderr, root, target, cargo_home, vendor, toolchain, sdk, declared, run_env
        )
        records, documents = compiler_inventory(stderr, root, target, cargo_home, vendor, toolchain, sdk, declared, producers)
        observed = {item[1] for row in records for item in row["inputs"] if item[0] == "workspace"}
        require(set(sources) <= observed, f"tracked Rust paths missing from {mode} dep-info: {set(sources) - observed}")
        build = {
            "command": [str(arg) for arg in command],
            "artifacts": artifacts,
            "rustcInputs": records,
            "rustdoc": documents,
            "buildScriptFacts": build_facts,
            "buildScriptRunHashes": run_hashes,
            "artifactProfiles": sorted([{
                "target": {key: value for key, value in row["target"].items() if key != "src_path"},
                "profile": row["profile"],
                "features": row["features"],
            } for row in artifacts], key=lambda row: json.dumps(row, sort_keys=True)),
            "docTestNames": [],
            "docTestOutcomes": {},
            "suites": {},
        }
        if mode == "test":
            for artifact in artifacts:
                if artifact["executable"] is None:
                    continue
                target_name = artifact["target"]["name"]
                executable = Path(artifact["executable"])
                require(executable.resolve().is_relative_to(target), f"test executable escaped target: {executable}")
                require(target_name not in build["suites"], f"duplicate test target {target_name}")
                actual_env = [assignments for words in cargo_words(stderr)
                              for assignments, argv in [cargo_environment(words)]
                              if argv == [str(executable)]]
                require(len(actual_env) == 1, f"no unique Cargo execution for {executable}")
                build["suites"][target_name] = execute_suite(
                    Path(artifact["manifest_path"]).parent, dict(run_env, **actual_env[0]),
                    executable, output, f"test-{target_name}", sandbox
                )
            require(build["suites"], f"no test suite observed for {PACKAGE}")
            if documents:
                doc, _ = run([cargo, "test", *prefix, "--doc", "--", "--list"],
                             root, run_env, output, "doctest-list", sandbox)
                build["docTestNames"] = list_tests(doc)
                cargo_unit_tests = {name for suite in build["suites"].values() for name in suite["outcomes"]}
                build["docTestOutcomes"] = {k: v for k, v in named_results(stdout).items() if k not in cargo_unit_tests}
                require(set(build["docTestNames"]) == build["docTestOutcomes"].keys(),
                        "incomplete named doctest outcomes")
        report["builds"][mode] = build
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", type=Path)
    parser.add_argument("output", type=Path, help="new directory outside the workspace")
    parser.add_argument("--expected", type=Path, help="compare to the frozen upstream receipt")
    parser.add_argument("--cargo-home", type=Path, default=Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")))
    parser.add_argument("--vendor", type=Path, help="versioned Cargo vendor directory configured in --cargo-home")
    parser.add_argument("--sandbox", type=Path, help="apply a Seatbelt profile to each Cargo or test command")
    args = parser.parse_args()
    try:
        root = args.workspace.resolve(strict=True)
        output = args.output.resolve()
        require(not output.is_relative_to(root), "put receipt directory outside the workspace")
        output.mkdir()
        original = json.loads(args.expected.read_text()) if args.expected else None
        report = capture(root, output, original, args.cargo_home.resolve(strict=True),
                         args.sandbox.resolve(strict=True) if args.sandbox else None,
                         args.vendor.resolve(strict=True) if args.vendor else None)
        if original:
            compare(original, report)
        save(output / "receipt.json", report)
        print(output / "receipt.json")
    except (OSError, subprocess.CalledProcessError, ValueError) as e:
        print(e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
