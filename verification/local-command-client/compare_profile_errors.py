"""Compare local wait diagnostics in private disposable profiles on macOS."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import tempfile


CASES = (
    "missing-state", "missing-profile", "missing-config", "world-readable-state",
    "regular-file-state", "symlink-state", "world-readable-profile",
    "malformed-config", "world-readable-config",
)
LINK_CASES = ("symlink-profile", "regular-file-profile", "symlink-config", "hardlinked-config")
ARGS = ["--key-storage", "file", "--no-autostart", "--state-dir", "<state>",
        "--identity", "audit", "wait", "--handle", "{}"]


def fixture(root, case):
    state = root / "state"
    if case in LINK_CASES:
        state.mkdir(mode=0o700)
        profile = state / "audit"
        if case == "regular-file-profile":
            profile.touch(mode=0o600)
        elif case == "symlink-profile":
            target = root / "target"
            target.mkdir(mode=0o700)
            profile.symlink_to(target, target_is_directory=True)
        else:
            profile.mkdir(mode=0o700)
            target = root / "target"
            target.write_bytes(b"{")
            target.chmod(0o600)
            config = profile / "config.json"
            if case == "symlink-config":
                config.symlink_to(target)
            else:
                config.hardlink_to(target)
        return state
    if case == "missing-state":
        return state
    if case == "regular-file-state":
        state.touch(mode=0o600)
    elif case == "symlink-state":
        target = root / "target"
        target.mkdir(mode=0o700)
        state.symlink_to(target, target_is_directory=True)
    else:
        state.mkdir(mode=0o700)
        if case == "world-readable-state":
            state.chmod(0o755)
        elif case != "missing-profile":
            profile = state / "audit"
            profile.mkdir(mode=0o700)
            if case == "world-readable-profile":
                profile.chmod(0o755)
            elif case in ("malformed-config", "world-readable-config"):
                config = profile / "config.json"
                config.write_bytes(b"{")
                config.chmod(0o644 if case == "world-readable-config" else 0o600)
    return state


def observe(binary, case):
    with tempfile.TemporaryDirectory(dir="/private/tmp") as directory:
        state = fixture(Path(directory), case)
        env = {"HOME": directory, "TMPDIR": directory, "PATH": "/usr/bin:/bin", "LC_ALL": "C"}
        policy = ('(version 1)(allow default)(deny network*)(deny mach-lookup)'
                  '(deny file-write*)(allow file-write* (subpath ' + json.dumps(directory) + '))')
        argv = [str(state) if arg == "<state>" else arg for arg in ARGS]
        result = subprocess.run(
            ["/usr/bin/sandbox-exec", "-p", policy, binary, *argv], input=b"",
            cwd=directory, env=env, capture_output=True, timeout=3,
        )
        return {"exit": result.returncode, "stdout": result.stdout.decode("ascii"),
                "stderr": result.stderr.decode("ascii")}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mainline", type=Path)
    parser.add_argument("sealed_export", type=Path)
    parser.add_argument("--include-link-cases", action="store_true")
    parser.add_argument("--require-match", action="store_true",
                        help="exit unsuccessfully if either stream or the status differs")
    args = parser.parse_args()
    if (platform.system(), platform.machine()) != ("Darwin", "arm64"):
        parser.error("this fixture requires ARM64 macOS")
    binaries = {"mainline": args.mainline.resolve(strict=True),
                "generated": (args.sealed_export / "marmish").resolve(strict=True)}
    report = {"platform": "aarch64-apple-darwin",
              "workspaceLockSha256": sha256(args.sealed_export / "workspace.lock"),
              "binarySha256": {name: sha256(path) for name, path in binaries.items()},
              "argv": ARGS, "cases": {}}
    cases = CASES + LINK_CASES if args.include_link_cases else CASES
    for case in cases:
        report["cases"][case] = {name: observe(path, case) for name, path in binaries.items()}
    print(json.dumps(report, indent=2))
    if args.require_match:
        differences = [case for case, pair in report["cases"].items()
                       if pair["mainline"] != pair["generated"]]
        if differences:
            print("Different results for " + ", ".join(differences), file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
