"""Qualify a source-free Marmish stream checkout on native Apple Silicon."""
import argparse
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import subprocess
import sys
import tarfile

from stream_baseline import command_output as capture
from stream_baseline import require, save, sha
from stream_baseline import LOGICAL_ROOT, ORIGINAL, PACKAGE, PACKAGE_ROOT, RUST, RUST_COMMIT
from stream_baseline import list_tests
from stream_baseline import capture as stream_capture
from stream_baseline import compare as stream_compare

HERE = Path(__file__).resolve().parent
ORACLE = HERE.parent.parent / "examples/marmish/stream-driver"
CONSUMERS = ("marmish-engine", "marmish-prototype", "marmish-native")
CONSUMER_CASES = {
    "marmish-engine/lib/marmish_engine": (
        "copy_launch::tests::owned_context_is_read_only_cloexec_above_three_and_has_no_writer",
    ),
    "marmish-prototype/test/bridge": ("framing_is_bounded_and_diagnostics_are_sanitized",),
    "marmish-native/lib/marmish_native": (
        "copy_io::tests::coalesced_prefix_and_data_are_bounded_and_delivered_in_order",
        "copy_worker::tests::sender_data_record_fits_one_transport_block",
        "stream_runner::tests::failed_exchange_does_not_apply_other_channel_ack",
        "stream_session::tests::output_ack_regression_is_rejected_without_an_ipc_request",
    ),
}
HEX256 = re.compile(r"[0-9a-f]{64}\Z")
TEST_SUMMARY = re.compile(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
                          re.MULTILINE)


def closure_request(op, **values):
    manifest = HERE.parents[1] / "verification/provider-validation/Cargo.toml"
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--locked", "--offline", "--manifest-path", manifest,
         "--bin", "build-closure", "--", "request"],
        input=json.dumps(dict(op=op, **values), default=os.fspath), text=True,
        stdout=subprocess.PIPE, check=True,
    )
    return json.loads(result.stdout)


def profile(trees, writable, literals):
    return closure_request("profile", trees=trees, writable=writable, literals=literals)


def checked_run(command, name, directory, env, output, seatbelt=None, expected_success=True):
    return closure_request("run", command=command, name=name, dir=directory, env=env,
                           out=output, sandbox=seatbelt, success=expected_success)


def prepare_vendor(cargo, src, vendor, env, output, trees, literals):
    closure_request("vendor", cargo=cargo, src=src, dest=vendor, env=env, out=output,
                    trees=trees, literals=literals)


def digest(path):
    return sha(path.read_bytes())


def git_archive(repo, commit, output):
    require(re.fullmatch(r"[0-9a-f]{40}", commit), "expected a full Git commit ID")
    require(capture(["git", "-C", repo, "rev-parse", f"{commit}^{{commit}}"]) == commit,
            "revision is not a commit")
    archive = subprocess.check_output(["git", "-C", repo, "archive", "--format=tar", commit])
    output.mkdir()
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tar:
        entries = set()
        for member in tar.getmembers():
            name = PurePosixPath(member.name)
            require(name.as_posix() == member.name and not name.is_absolute() and
                    ".." not in name.parts and ".git" not in name.parts and member.name not in entries,
                    f"unsafe Git archive member {member.name}")
            require(member.isfile() or member.isdir(), f"special Git archive member {member.name}")
            entries.add(member.name)
        tar.extractall(output, filter="data")
    return sha(archive)


def tool_paths():
    cargo = Path(capture(["rustup", "which", "--toolchain", RUST, "cargo"])).resolve()
    toolchain = cargo.parents[1]
    rustc_version = capture([toolchain / "bin/rustc", "-vV"])
    require(f"commit-hash: {RUST_COMMIT}" in rustc_version, "unexpected Rust compiler")
    sdk = Path(capture(["xcrun", "--show-sdk-path"])).resolve()
    clang = Path(capture(["xcrun", "--find", "clang"])).resolve()
    developer_bin = Path(capture(["xcrun", "--find", "make"])).resolve().parent
    return toolchain, sdk, clang, developer_bin


def native_identity(toolchain, sdk, clang, developer_bin):
    linker = Path(capture(["xcrun", "--find", "ld"])).resolve()
    return {
        "rustc": capture([toolchain / "bin/rustc", "-vV"]),
        "clang": {"path": str(clang), "sha256": digest(clang), "version": capture([clang, "--version"])},
        "linker": {"path": str(linker), "sha256": digest(linker)},
        "sdk": {"path": str(sdk), "version": capture(["xcrun", "--show-sdk-version"]),
                "settingsSha256": digest(sdk / "SDKSettings.json")},
        "developerBin": str(developer_bin),
        "macOS": platform.mac_ver()[0],
    }


def boundaries(toolchain, sdk, clang, developer_bin):
    system = [Path(p) for p in (
        "/System/Library", "/usr/lib", "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
        "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
    )]
    trees = [toolchain, sdk, clang.parents[2], developer_bin.parent / "lib", Path("/dev/fd"), *system]
    files = ["/bin/" + name for name in ("sh", "bash", "cat", "cp", "echo", "mkdir", "mv", "rm", "ln")]
    files += ["/usr/bin/" + name for name in (
        "env", "git", "perl", "perl5.34", "awk", "sed", "grep", "tr", "touch", "uname",
        "dirname", "basename", "find", "xargs", "cmp", "sort", "expr", "head", "tail", "mktemp",
        "diff",
    )]
    files += [developer_bin / name for name in ("make", "git")]
    files += ["/", "/private/etc/ssl/openssl.cnf", "/dev/null", "/dev/zero", "/dev/urandom",
              "/dev/random", ORACLE / "main.rs"]
    return trees, files


def clean_environment(writable, root, toolchain, sdk, clang, developer_bin):
    writable.mkdir()
    home, cargo_home, temp = (writable / rel for rel in ("home", "cargo", "tmp"))
    for path in (home, cargo_home, temp):
        path.mkdir()
    remap = f"--remap-path-prefix={root}={LOGICAL_ROOT}"
    return {
        "HOME": str(home), "CARGO_HOME": str(cargo_home), "TMPDIR": str(temp),
        "PATH": f"{toolchain}/bin:{clang.parent}:{developer_bin}:/usr/bin:/bin",
        "SDKROOT": str(sdk), "CC": str(clang), "CXX": str(clang.parent / "clang++"),
        "RUSTC": str(toolchain / "bin/rustc"), "RUSTDOC": str(toolchain / "bin/rustdoc"),
        "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": str(clang),
        "CARGO_ENCODED_RUSTFLAGS": remap, "CARGO_ENCODED_RUSTDOCFLAGS": remap,
        "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never", "OPENSSL_CONF": "/dev/null",
        "MARMISH_BUILD_COMMIT": ORIGINAL, "LC_ALL": "C", "TZ": "UTC",
    }


def git_object_sample(repo, rel):
    blob = capture(["git", "-C", repo, "rev-parse", f"HEAD:{rel}"])
    common = Path(capture(["git", "-C", repo, "rev-parse", "--path-format=absolute", "--git-common-dir"]))
    objects = common / "objects"
    loose = objects / blob[:2] / blob[2:]
    if loose.is_file():
        return loose
    packs = sorted((objects / "pack").glob("*.pack"))
    require(packs, f"cannot find any on-disk object storage for {repo}")
    return packs[0]


def denied_reads(sandbox, paths, work, env):
    observations = {}
    for name, path in paths.items():
        with path.open("rb") as file:
            require(file.read(1), f"empty isolation canary {name}")
        command = ["/usr/bin/sandbox-exec", "-f", sandbox, "/bin/cat", path]
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True)
        require(result.returncode != 0 and not result.stdout and
                "Operation not permitted" in result.stderr, f"read isolation probe failed: {name}")
        observations[name] = {"path": str(path), "returnCode": result.returncode,
                              "stderr": result.stderr.strip()}
    return observations


def declared_bundle(root, relative, pin):
    name = PurePosixPath(relative)
    require(name.as_posix() == relative and not name.is_absolute() and ".." not in name.parts,
            "bundle must be a normalized path inside the committed checkout")
    bundle = (root / relative).resolve(strict=True)
    require(bundle.is_relative_to(root) and bundle.is_dir(), "bundle must be a directory in the checkout")
    inputs = [bundle / file for file in ("bundle.lock", "sources.json", "syntax.ttl")]
    require(all(path.is_file() and not path.is_symlink() for path in inputs), "missing bundle inputs")
    require(HEX256.fullmatch(pin) and digest(inputs[0]) == pin, "bundle lock digest mismatches pin")
    return bundle, inputs


def emitted_sources(path, expected):
    found = {file.relative_to(path).as_posix(): file for file in path.rglob("*") if not file.is_dir()}
    require({PACKAGE_ROOT + name for name in found} == expected.keys(),
            f"emitted tracked Rust paths differ: {sorted(found)}")
    copied = {}
    for name, file in sorted(found.items()):
        target = PACKAGE_ROOT + name
        require(file.is_file() and not file.is_symlink() and
                digest(file) == expected[target]["sha256"], f"emitted Rust digest changed: {target}")
        copied[target] = file
    return copied


def manifest_inputs(root, expected):
    for name, pin in expected.items():
        require(digest(root / name) == pin, f"candidate changed baseline Cargo input {name}")


def regular_inputs(root):
    return {file.relative_to(root).as_posix() for file in root.rglob("*") if file.is_file()}


def run_capture(command, stem, cwd, env, sandbox, logs):
    bound = ["/usr/bin/sandbox-exec", "-f", sandbox, *command]
    with (logs / f"{stem}.stdout").open("wb") as out, (logs / f"{stem}.stderr").open("wb") as err:
        return subprocess.run(bound, cwd=cwd, env=env, stdout=out, stderr=err).returncode


def test_outcome(executable, name, stem, directory, env, sandbox, logs):
    rc = run_capture([executable, "--test-threads=1", "--exact", name], stem, directory,
                     env, sandbox, logs)
    matches = TEST_SUMMARY.findall((logs / f"{stem}.stdout").read_text())
    require(matches and rc in (0, 101), f"test did not report a libtest outcome: {name} ({rc})")
    counts = tuple(map(int, matches[-1]))
    require(sum(counts) == 1 and (rc == 101) == (counts[1] == 1),
            f"ambiguous test outcome: {name} ({rc}, {counts})")
    return ("ok", "FAILED", "ignored")[counts.index(1)]


def consumer_tests(root, writable, env, sandbox, logs, cargo, check_workspace):
    logs.mkdir()
    target = writable / "consumers"
    scoped_env = dict(env, CARGO_TARGET_DIR=str(target))
    common = ["--locked", "--offline"]
    commands = {}
    if check_workspace:
        command = [cargo, "check", *common, "--workspace", "--all-targets"]
        checked_run(command, "workspace-check", root, scoped_env, logs, sandbox)
        commands["workspaceCheck"] = list(map(str, command))
    selector = [value for name in CONSUMERS for value in ("-p", name)]
    command = [cargo, "test", *common, *selector, "--message-format=json", "--no-run"]
    checked_run(command, "consumer-build", root, scoped_env, logs, sandbox)
    commands["consumerBuild"] = list(map(str, command))
    runtime = logs / "runtime.sb"
    consumer_profile(sandbox, runtime)
    suites = consumer_observations(target, scoped_env, runtime, logs,
                                   logs / "consumer-build.stdout", check_workspace)
    return {"commands": commands, "suites": suites}


def consumer_profile(build_profile, runtime_profile):
    # Native runtime tests resolve the local Unix account with getpwuid_r.
    # OpenDirectory is not needed for emission or compilation.
    runtime_profile.write_text(build_profile.read_text() +
                               '(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))\n')


def consumer_observations(target, env, sandbox, logs, artifacts, allow_fresh):
    suites = {}
    for line in artifacts.read_text().splitlines():
        record = json.loads(line)
        if (record["reason"] != "compiler-artifact" or not record["executable"]
                or not record["profile"]["test"]):
            continue
        require(not record["fresh"] or allow_fresh, "consumer executable unexpectedly reused")
        executable = Path(record["executable"]).resolve()
        require(executable.is_relative_to(target), f"unexpected executable path {executable}")
        crate_root = Path(record["manifest_path"]).parent
        name = crate_root.name
        require(name in CONSUMERS, f"unexpected consumer {name}")
        key = name + "/" + ":".join(record["target"]["kind"]) + "/" + record["target"]["name"]
        require(key not in suites, f"ambiguous consumer suite {key}")
        stem = f"suite-{len(suites)}"
        binary_sha = digest(executable)
        suite_env = dict(env, CARGO_MANIFEST_DIR=str(crate_root), CARGO_PKG_NAME=name)
        checked_run([executable, "--list"], stem + "-list", crate_root, suite_env, logs, sandbox)
        checked_run([executable, "--list", "--ignored"], stem + "-ignored", crate_root,
                    suite_env, logs, sandbox)
        require(digest(executable) == binary_sha, f"consumer executable changed {key}")
        listed = list_tests((logs / f"{stem}-list.stdout").read_text())
        ignored = list_tests((logs / f"{stem}-ignored.stdout").read_text())
        selected = CONSUMER_CASES.get(key, ())
        require(set(selected) <= set(listed) and not set(selected).intersection(ignored),
                f"required test is missing or ignored: {key}")
        # Some tests spawn the same executable with their own filters, interleaving
        # nested libtest output with the outer test name. Observe one outer final
        # summary per selected name so nested output cannot supply its result.
        outcomes = {name: test_outcome(executable, name, f"{stem}-{i}", crate_root,
                                       suite_env, sandbox, logs)
                    for i, name in enumerate(selected)}
        require(all(value == "ok" for value in outcomes.values()), f"required consumer test failed: {key}")
        suites[key] = {"path": str(executable), "sha256": binary_sha, "list": listed,
                       "ignored": ignored, "selected": selected, "outcomes": outcomes,
                       "logs": stem}
    require({key.split("/")[0] for key in suites} == set(CONSUMERS), "missing consumer test targets")
    require(CONSUMER_CASES.keys() <= suites.keys(), "missing required consumer test suites")
    return suites


def compare_consumers(original, candidate):
    require(original["suites"].keys() == candidate["suites"].keys(), "consumer suites changed")
    for name, left in original["suites"].items():
        right = candidate["suites"][name]
        for field in ("list", "ignored", "selected", "outcomes"):
            require(left[field] == right[field], f"drop-in test comparison differs: {name}.{field}")


def oracle_binary():
    manifest = HERE.parent.parent / "verification/marmish-inventory/Cargo.toml"
    build = subprocess.run(
        ["cargo", "+1.97.1", "build", "--locked", "--offline", "--manifest-path", manifest,
         "--bin", "stream-driver", "--message-format=json-render-diagnostics"],
        check=True, stdout=subprocess.PIPE, text=True,
    )
    executables = [row["executable"] for line in build.stdout.splitlines()
                   if (row := json.loads(line))["reason"] == "compiler-artifact"
                   and row["target"]["name"] == "stream-driver" and row.get("executable")]
    require(len(executables) == 1, "missing native stream runner")
    return executables[0]


def run_oracle(root, writable, env, sandbox, toolchain, role, peer, output, baseline,
               commit, archive, bundle=None):
    command = [oracle_binary(), "--workspace", root, "--target-dir",
               writable / "oracle-target", "--receipt", output, "--role", role,
               "--toolchain", toolchain, "--sandbox", sandbox, "--peer-receipt", peer,
               "--baseline", baseline, "--commit", commit, "--archive-sha256", archive]
    if bundle:
        command += ["--bundle-sha256", bundle]
    # The host reads the corpus and receipts. The selected profile confines Cargo,
    # rustc and every linked observer process.
    subprocess.run(command, env=env, cwd=root, check=True)


def controls(root, writable, env, sandbox, vendor, cargo, baseline, output):
    target = writable / "negative-target"
    env = dict(env, CARGO_TARGET_DIR=str(target))
    module = root / PACKAGE_ROOT / "src/copy_protocol.rs"
    contents = module.read_bytes()
    module.unlink()
    cargo_args = [cargo, "check", "--locked", "--offline", "-p", PACKAGE]
    try:
        rc = run_capture(cargo_args, "missing-module", root, env, sandbox, output)
        message = (output / "missing-module.stderr").read_text()
        require(rc == 101 and "file not found for module `copy_protocol`" in message,
                "missing expected compile error for withdrawn copy_protocol.rs")
    finally:
        module.write_bytes(contents)
    marker = root / PACKAGE_ROOT / "src/undeclared-input"
    lib = root / PACKAGE_ROOT / "src/lib.rs"
    original = lib.read_bytes()
    marker.write_bytes(b"oddlang-independent-undeclared-input")
    lib.write_bytes(original + b'\nconst _: &[u8] = include_bytes!("undeclared-input");\n')
    report = writable / "negative-baseline"
    report.mkdir()
    try:
        try:
            stream_capture(root, report, baseline, Path(env["CARGO_HOME"]), sandbox, vendor)
        except ValueError as failure:
            expected = f"undeclared compiler input {PACKAGE_ROOT}src/undeclared-input"
            require(str(failure) == expected, f"unexpected undeclared input result: {failure}")
        else:
            raise ValueError("qualification allowed an undeclared compiler input")
    finally:
        lib.write_bytes(original)
        marker.unlink()
    return {"missingModule": rc, "undeclaredInput": expected}


def qualify(args):
    require((platform.system(), platform.machine()) == ("Darwin", "arm64"),
            "qualification requires native Apple Silicon macOS")
    args.output.mkdir()
    output = args.output.resolve()
    for repo in (args.repository, args.original):
        require(not output.is_relative_to(repo), "evidence must be outside both checkouts")
    oddlang_sha = digest(args.oddlang)
    baseline = json.loads(args.baseline.read_text())
    expected = baseline["trackedSources"]
    require(baseline["upstreamCommit"] == ORIGINAL and expected, "wrong frozen stream baseline")
    oracle = json.loads(args.oracle_original.read_text())
    require(oracle["upstream_commit"] == ORIGINAL and oracle["role"] == "original" and
            oracle["all_matched"] and oracle["workspace_identity"]["baselineSha256"] == digest(args.baseline),
            "wrong original stream oracle receipt")

    src = output / "candidate"
    archive_sha = git_archive(args.repository, args.commit, src)
    manifest_inputs(src, baseline["cargoInputs"])
    with_rust = regular_inputs(src / PACKAGE_ROOT)
    require(not any(name.endswith(".rs") for name in with_rust),
            "downstream commit contains original Rust source in the stream package")
    bundle, graph_inputs = declared_bundle(src, args.bundle, args.bundle_sha256)
    require(len(args.deny_read) >= 1, "pass at least one retained artifact as a denial control")
    undeclared = output / "outside-sandbox"
    undeclared.write_bytes(b"outside candidate and writable build roots\n")
    original_source = next(iter(expected))
    denied = {"originalSource": args.original / original_source,
              "originalObjectStore": git_object_sample(args.original, original_source),
              "candidateObjectStore": git_object_sample(args.repository, "Cargo.toml"),
              "unlistedInput": undeclared}
    denied.update({f"retainedArtifact{i}": path for i, path in enumerate(args.deny_read)})

    tools = tool_paths()
    native = native_identity(*tools)
    toolchain = tools[0]
    cargo = toolchain / "bin/cargo"
    trees, files = boundaries(*tools)
    candidate_work = output / "a"
    env = clean_environment(candidate_work, src, *tools)
    emitter = output / "emitter"
    emitter.mkdir()
    emit_profile = output / "emit.sb"
    emit_profile.write_text(profile(trees, emitter, [*files, args.oddlang, *graph_inputs]))
    observed = {"emitter": denied_reads(emit_profile, denied, emitter, env)}
    command = [args.oddlang, "build", "rust-tree", bundle, emitter / "generated",
               "--bundle-sha256", args.bundle_sha256]
    checked_run(command, "emit", emitter, env, output, emit_profile)
    generated = emitted_sources(emitter / "generated", expected)
    bootstrap = src / "oddlang/regenerate-stream.sh"
    bootstrap_pin = src / "oddlang/stream.sha256"
    require(bootstrap_pin.read_text().strip() == args.bundle_sha256, "bootstrap uses a different pin")
    bootstrap_profile = output / "bootstrap.sb"
    bootstrap_profile.write_text(profile(
        trees, src / PACKAGE_ROOT,
        [*files, src, src / "oddlang", *src.parents, args.oddlang, bootstrap, bootstrap_pin, *graph_inputs],
    ))
    observed["bootstrap"] = denied_reads(bootstrap_profile, denied, src, env)
    bootstrap_env = dict(env, ODDLANG=str(args.oddlang))
    checked_run(["/bin/bash", bootstrap], "bootstrap", src, bootstrap_env, output, bootstrap_profile)
    checked_run(["/bin/bash", bootstrap], "bootstrap-replay", src, bootstrap_env, output,
                bootstrap_profile)
    require(digest(args.oddlang) == oddlang_sha, "Oddlang emitter changed during regeneration")
    for relative, path in generated.items():
        require(digest(src / relative) == digest(path), f"bootstrap output differs: {relative}")
    require({PACKAGE_ROOT + name for name in regular_inputs(src / PACKAGE_ROOT) if name.endswith(".rs")}
            == expected.keys(), "candidate Rust tree differs from emitted inventory")

    vendor = output / "preparation/vendor"
    prepare_vendor(cargo, src, vendor, env, output, [*trees, src], files)
    configuration = (output / "vendor.stdout").read_bytes()
    (Path(env["CARGO_HOME"]) / "config.toml").write_bytes(configuration)
    build = output / "build.sb"
    # Deny a build script access to the syntax input once the independent emitter
    # has published its verified tree.
    build.write_text(profile([*trees, src, vendor], candidate_work, [*files, *candidate_work.parents]) +
                     f"(allow network* (subpath {json.dumps(str(candidate_work))}))\n" +
                     f"(deny file-read-data (subpath {json.dumps(str(bundle))}))\n")
    observed["candidateBuild"] = denied_reads(build, denied | {"graph": graph_inputs[-1]}, src, env)
    save(output / "denied-reads.json", observed)
    baseline_output = candidate_work / "stream"
    baseline_output.mkdir()
    candidate = stream_capture(src, baseline_output, baseline, Path(env["CARGO_HOME"]), build, vendor)
    save(baseline_output / "receipt.json", candidate)
    stream_compare(baseline, candidate)
    print("generated stream package matches frozen source, inputs and tests", flush=True)

    consumers = consumer_tests(src, candidate_work, env, build, output / "candidate-consumers",
                               cargo, check_workspace=True)
    save(output / "candidate-consumers.json", consumers)
    run_oracle(src, candidate_work, env, build, toolchain, "candidate", args.oracle_original,
               output / "candidate-oracle.json", args.baseline, args.commit, archive_sha,
               args.bundle_sha256)
    negative = controls(src, candidate_work, env, build, vendor, cargo, baseline, output)

    original = output / "upstreams"
    original_sha = git_archive(args.original, ORIGINAL, original)
    original_work = output / "b"
    original_env = clean_environment(original_work, original, *tools)
    (Path(original_env["CARGO_HOME"]) / "config.toml").write_bytes(configuration)
    original_profile = output / "original.sb"
    original_profile.write_text(profile([*trees, original, vendor], original_work,
                                        [*files, *original_work.parents]) +
                                f"(allow network* (subpath {json.dumps(str(original_work))}))\n")
    original_consumers = consumer_tests(original, original_work, original_env, original_profile,
                                        output / "original-consumers", cargo, check_workspace=False)
    save(output / "original-consumers.json", original_consumers)
    compare_consumers(original_consumers, consumers)
    run_oracle(original, original_work, original_env, original_profile, toolchain, "original",
               output / "candidate-oracle.json", output / "reverse-oracle.json",
               args.baseline, ORIGINAL, original_sha)
    save(output / "qualification.json", {
        "version": 1, "marmishOddlangCommit": args.commit, "archiveSha256": archive_sha,
        "originalCommit": ORIGINAL, "originalArchiveSha256": original_sha,
        "sourceArchiveRustPaths": [], "generatedPaths": sorted(generated),
        "bundleSha256": args.bundle_sha256, "syntaxGraphSha256": digest(graph_inputs[-1]),
        "oddlangSha256": oddlang_sha, "originalBaselineSha256": digest(args.baseline),
        "nativeToolchain": native,
        "consumerScope": {key: list(cases) for key, cases in CONSUMER_CASES.items()},
        "negativeControls": negative,
        "evidenceSha256": {str(path.relative_to(output)): digest(path) for path in (
            emit_profile, bootstrap_profile, build, original_profile, baseline_output / "receipt.json",
            output / "denied-reads.json", output / "locked-packages.json",
            output / "candidate-consumers.json", output / "original-consumers.json",
            output / "candidate-consumers/runtime.sb", output / "original-consumers/runtime.sb",
            output / "candidate-oracle.json", output / "reverse-oracle.json",
        )},
    })
    print(output / "qualification.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("repository", "oddlang", "original", "baseline", "oracle-original", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("commit", "bundle", "bundle-sha256"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--deny-read", action="append", type=Path, required=True,
                        help="prior source or build artifacts that must be unreadable")
    args = parser.parse_args()
    for attr in ("repository", "oddlang", "original", "baseline", "oracle_original"):
        setattr(args, attr, getattr(args, attr).resolve(strict=True))
    args.deny_read = [path.resolve(strict=True) for path in args.deny_read]
    try:
        qualify(args)
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
