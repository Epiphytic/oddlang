#!/usr/bin/env python3
"""Observe a disposable checkout of Marmish's frozen upstream revision on Apple Silicon."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import tempfile

PIN = "49fc705408a230bd0ad896248d08dbdd2faad05a"
TARGET = "aarch64-apple-darwin"
TOOLCHAIN = "1.97.1"
BINS = ("marmish", "marmishd", "marmish-worker")
PROFILE = Path(__file__).with_name("baseline-macos.sb")


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def output(args):
    return subprocess.check_output(args, text=True).strip()


def run(upstream, report, tmp):
    if output(["git", "-C", upstream, "rev-parse", "HEAD"]) != PIN:
        raise SystemExit("upstream revision does not match pinned Marmish")
    source = tmp / "source"
    home = tmp / "home"
    temp = tmp / "tmp"
    cargo = tmp / "cargo"
    for directory in (home, temp, cargo, report / "logs"):
        directory.mkdir(mode=0o700)
    # Clone tracked inputs only. Do not reuse a previously compiled executable.
    subprocess.run(["git", "-c", "init.templateDir=", "clone", "--no-local", "--quiet",
                    "--no-checkout", str(upstream), str(source)], check=True)
    subprocess.run(["git", "-C", source, "-c", "core.hooksPath=/dev/null", "checkout",
                    "--quiet", "--detach", PIN], check=True)
    tools = {"cargo": Path(output(["rustup", "which", "--toolchain", TOOLCHAIN, "cargo"])),
             "uv": Path(shutil.which("uv")).resolve()}
    personal = Path.home().resolve()
    package_cache = Path(os.environ.get("CARGO_HOME", personal / ".cargo")).resolve()
    for name in ("registry", "git"):
        (cargo / name).symlink_to(package_cache / name, target_is_directory=True)
    python = Path(output(["uv", "python", "find", "--offline", "3.12"])).resolve()
    # Re-use the immutable wheel/source cache. Each run installs a fresh venv.
    # uv expects writable bucket markers but may read the underlying cached objects.
    uv_cache = Path(output([tools["uv"], "cache", "dir"])).resolve()
    run_cache = tmp / "uv-cache"
    run_cache.mkdir()
    (run_cache / "archive-v0").symlink_to(uv_cache / "archive-v0")
    for prefix in ("wheels", "sdists", "git", "simple"):
        for bucket in uv_cache.glob(prefix + "-v[0-9]*"):
            local = run_cache / bucket.name
            local.mkdir()
            for item in bucket.iterdir():
                if not item.name.startswith(".") and item.name != "locks":
                    (local / item.name).symlink_to(item)
    params = dict(WORK=tmp, PERSONAL=personal, CARGO_REGISTRY=package_cache / "registry",
                  CARGO_GIT=package_cache / "git", TOOLCHAIN=tools["cargo"].parents[1],
                  PYTHON=python.parents[1], UV=tools["uv"], UV_CACHE=uv_cache)
    policy = ["/usr/bin/sandbox-exec"]
    for name, path in params.items():
        policy += ["-D", f"{name}={path}"]
    captured_profile = report / "baseline-macos.sb"
    shutil.copyfile(PROFILE, captured_profile)
    policy += ["-f", str(captured_profile)]
    xcode = Path(output(["xcrun", "--find", "clang"])).parent
    env = dict(PATH=f"{tools['cargo'].parent}:{xcode}:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
               HOME=str(home), TMPDIR=str(temp), CARGO_HOME=str(cargo),
               CARGO_TARGET_DIR=str(tmp / "target"), CARGO_NET_OFFLINE="true",
               UV_OFFLINE="true", UV_CACHE_DIR=str(tmp / "uv-cache"), UV_PYTHON=str(python),
               UV_PYTHON_DOWNLOADS="never", UV_LINK_MODE="copy",
               SDKROOT=output(["xcrun", "--show-sdk-path"]), GIT_CONFIG_NOSYSTEM="1", LC_ALL="C", TZ="UTC",
               RUST_TEST_THREADS="1", GIT_CONFIG_GLOBAL="/dev/null")
    state = dict(revision=PIN, tree=output(["git", "-C", source, "rev-parse", "HEAD^{tree}"]),
                 host=TARGET, osVersion=platform.mac_ver()[0], tools={}, commands=[], binaries={},
                 platforms={TARGET: "observed", "aarch64-unknown-linux-gnu": "unobserved",
                            "x86_64-unknown-linux-gnu": "unobserved"},
                 locks={f: sha(source / f) for f in ("Cargo.lock", "rust-toolchain.toml",
                                                      "tools/code-ontology/uv.lock")},
                 isolation={"profileSha256": sha(captured_profile), "network": "Seatbelt denies IP networking; allows run-local Unix sockets",
                            "workspace": "new local clone of pinned Git tree",
                            "target": "empty disposable build directory",
                            "state": "disposable HOME, TMPDIR, Cargo and uv working directories"},
                 timeoutSeconds=900, runnerSha256=sha(Path(__file__)),
                 flags={"RUSTFLAGS": "", "RUST_TEST_THREADS": "1", "CARGO_NET_OFFLINE": "true",
                        "SDKROOT": env["SDKROOT"], "LC_ALL": "C", "TZ": "UTC",
                        "UV_OFFLINE": "true", "CARGO_BUILD_TARGET": "host default"})
    def save():
        (report / "baseline.json").write_text(json.dumps(state, sort_keys=True, indent=2) + "\n")
    capture = tmp / "logs"
    capture.mkdir(mode=0o700)

    def observe(name, args):
        stdout = report / "logs" / (name + ".stdout")
        stderr = report / "logs" / (name + ".stderr")
        print(name, flush=True)
        # Marmish verifies the enclosing directory of each inherited vnode descriptor.
        # Keep stdin and both capture files accessible under the same isolation rules.
        with (capture / stdout.name).open("wb") as out, (capture / stderr.name).open("wb") as err:
            with subprocess.Popen(policy + list(map(str, args)), cwd=source, env=env,
                                  stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True) as process:
                try:
                    code = process.wait(timeout=900)
                    timed_out = False
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    code = process.wait()
                    timed_out = True
        shutil.copyfile(capture / stdout.name, stdout)
        shutil.copyfile(capture / stderr.name, stderr)

        def portable(a):
            return str(a).replace(str(tmp), "$RUN").replace(str(tools["uv"]), "uv")
        entry = dict(name=name, argv=list(map(portable, args)), exitCode=code, timedOut=timed_out,
                     status="timed-out" if timed_out else "passed" if code == 0 else "failed",
                     stdout=str(stdout.relative_to(report)), stdoutSha256=sha(stdout),
                     stderr=str(stderr.relative_to(report)), stderrSha256=sha(stderr))
        state["commands"].append(entry)
        save()
        return entry, stdout
    for tool, args in (("rustc", ["rustc", "-vV"]), ("cargo", ["cargo", "-Vv"]),
                       ("uv", [tools["uv"], "--version"]), ("python", [python, "--version"]),
                       ("git", ["git", "--version"]), ("clang", ["clang", "--version"]),
                       ("sdk", ["xcrun", "--show-sdk-version"])):
        entry, log = observe(tool, args)
        if entry["exitCode"]:
            raise SystemExit(f"failed to capture {tool}; inspect {entry['stderr']}")
        state["tools"][tool] = log.read_text().strip()
    if TARGET not in state["tools"]["rustc"] or "release: 1.97.1" not in state["tools"]["rustc"]:
        raise SystemExit("toolchain/host mismatch")
    observe("cfg", ["rustc", "--print", "cfg", "--target", TARGET])
    observe("ontology", [tools["uv"], "run", "--locked", "--offline", "--python", str(python),
                         "--project", "tools/code-ontology", "python",
                         "tools/code-ontology/index.py", "check", "--ref", "HEAD"])
    def cargo_run(name, action, *args):
        return observe(name, ["cargo", action, "--locked", "--offline", *args])
    cargo_run("metadata", "metadata", "--format-version", "1")
    cargo_run("installed-metadata", "metadata", "--format-version", "1", "--features",
              "marmish-native/installed-checks")
    for name, args in (("workspace", ["--workspace"]), ("installed", ["-p", "marmish-native",
                                                               "--features", "installed-checks"])):
        entry, log = cargo_run(name, "test", *args, "--no-fail-fast")
        entry["summaries"] = re.findall(r"^test result: .*", log.read_text(), re.M)
        entry["failedTests"] = re.findall(r"^test (\S+) \.\.\. FAILED$", log.read_text(), re.M)
        entry["ignored"] = re.findall(r"^test (\S+) \.\.\. ignored(?:, (.*))?$", log.read_text(), re.M)
        # --list is separate so ignored tests remain enumerable even if an earlier test binary fails.
        cargo_run(name + "-ignored", "test", *args, "--", "--ignored", "--list")
    release, _ = cargo_run("release", "build", "--release", "-p", "marmish-native", "--bins")
    if release["exitCode"] == 0:
        helper = tmp / "target/release/marmish-release-tool"
        state["auxiliaryBinaries"] = {helper.name: {"sha256": sha(helper), "size": helper.stat().st_size}}
        for binary in BINS:
            path = tmp / "target/release" / binary
            state["binaries"][binary] = {"sha256": sha(path), "size": path.stat().st_size}
            for flag in ("version", "help"):
                observe(f"{binary}-{flag}", [path, "--" + flag])
    save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if (platform.system(), platform.machine()) != ("Darwin", "arm64"):
        raise SystemExit("this runner requires a native Apple Silicon macOS host")
    report = args.output.resolve()
    report.mkdir(parents=True, exist_ok=True)
    if any(report.iterdir()):
        raise SystemExit("output directory must be empty")
    with tempfile.TemporaryDirectory(prefix="m-", dir=Path(output(["getconf", "DARWIN_USER_TEMP_DIR"])).resolve()) as tmp:
        run(args.upstream.resolve(), report, Path(tmp))


if __name__ == "__main__":
    main()
