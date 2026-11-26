"""Check the assembled framing module and native build script in an offline macOS sandbox."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib

FRAME = "crates/marmish-core/src/frame.rs"
NATIVE_SCRIPT = "crates/marmish-native/build.rs"
PIN = "49fc705408a230bd0ad896248d08dbdd2faad05a"
TRIPLE = "aarch64-apple-darwin"
HOST = Path(__file__).resolve()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def capture(cmd):
    return subprocess.check_output(cmd, text=True).strip()


def stage_candidate(candidate, manifest, dest):
    entries = {}
    for entry in manifest["files"]:
        rel = entry["path"]
        require(PurePosixPath(rel).as_posix() == rel and not rel.startswith("/") and
                ".." not in PurePosixPath(rel).parts and rel not in entries, f"invalid path {rel}")
        entries[rel] = entry
    require({r for r, e in entries.items() if e["producer"] == "generated"} == {FRAME},
            "expected the generated framing module")
    require(entries[NATIVE_SCRIPT]["producer"] == "retained", "native build script must be retained")
    require(entries["Cargo.lock"]["sha256"] == manifest["cargoLockSha256"], "wrong workspace lock")
    found = set()
    for src in candidate.rglob("*"):
        mode = src.lstat().st_mode
        if stat.S_ISDIR(mode):
            continue
        rel = src.relative_to(candidate).as_posix()
        require(stat.S_ISREG(mode) and rel in entries, f"undeclared or special input: {rel}")
        content = src.read_bytes()
        row = entries[rel]
        require(sha(content) == row["sha256"] and
                (mode & 0o777) == int(row["mode"][-3:], 8), f"assembled bytes or mode differ: {rel}")
        target = dest / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
        target.chmod(mode & 0o777)
        found.add(rel)
    require(found == set(entries), "assembled tree is missing manifest inputs")
    return entries


def profile(read_trees, write_tree, literals):
    # JSON string quoting is also valid for these Scheme path string literals.
    def filters(kind, paths):
        return " ".join(f"({kind} {json.dumps(str(p))})" for p in paths)
    return f"""(version 1)
(deny default)
(allow file-read-metadata process-fork process-exec sysctl-read signal)
(allow process-info* (target self))
(allow file-map-executable {filters("subpath", [*read_trees, write_tree])} {filters("literal", literals)})
(deny network*)
(deny file-read-data file-write*)
(allow file-write* (literal "/dev/null"))
(allow file-read-data {filters('subpath', read_trees)} {filters('literal', literals)})
(allow file-read-data file-write* (subpath {json.dumps(str(write_tree))}) (literal "/dev/null"))
(deny mach-lookup (global-name-prefix "com.apple.securityd") (global-name "com.apple.SecurityServer"))
"""


def dep_paths(path):
    # rustc uses Make dep-info. Backslash escapes a space in a prerequisite name.
    line = path.read_text().replace("\\\n", "").splitlines()[0]
    _, inputs = line.split(": ", 1)
    return [re.sub(r"\\(.)", r"\1", name) for name in re.findall(r"(?:\\.|[^\s\\])+", inputs)]


def inside(path, root):
    return path == root or root in path.parents


def check_depinfo(candidate, target, vendor, toolchain, sdk, entries):
    records = []
    seen = set()
    for depfile in sorted(target.rglob("*.d")):
        # Cargo/rustc .d files carry target prerequisites. Native tool outputs in
        # OUT_DIR can use a different depfile grammar.
        if "out" in depfile.relative_to(target).parts:
            continue
        deps = []
        for item in dep_paths(depfile):
            p = (candidate / item).resolve()
            if inside(p, candidate):
                rel = p.relative_to(candidate).as_posix()
                require(rel in entries, f"undeclared compiler input {rel}")
                seen.add(rel)
                deps.append(["assembled", rel])
            else:
                for label, boundary in (("locked dependency", vendor), ("build output", target),
                                        ("toolchain", toolchain), ("SDK", sdk)):
                    if inside(p, boundary):
                        deps.append([label, p.relative_to(boundary).as_posix()])
                        break
                else:
                    raise ValueError(f"compiler dep-info escaped declared roots: {p}")
        records.append({"file": str(depfile.relative_to(target)), "inputs": deps})
    require(FRAME in seen, "compiler did not report generated frame.rs")
    require(NATIVE_SCRIPT in seen, "compiler did not report retained native build.rs")
    return records, seen


def checked_run(command, name, directory, env, output, seatbelt=None, expected_success=True):
    args = ["/usr/bin/sandbox-exec", "-f", seatbelt, *command] if seatbelt else command
    with (output / (name + ".stdout")).open("wb") as stdout, (output / (name + ".stderr")).open("wb") as stderr:
        rc = subprocess.run(args, cwd=directory, env=env, stdin=subprocess.DEVNULL,
                            stdout=stdout, stderr=stderr).returncode
    require((rc == 0) == expected_success, f"{name} exited {rc}; see {output / (name + '.stderr')}")
    return rc


def prepare_vendor(cargo, src, vendor, env, output, read_trees, literals):
    # The source-preparation process sees read-only package caches. None of those
    # cache paths is present in the profile used for compiling or running build.rs.
    preparation = vendor.parent
    preparation.mkdir()
    private_home = preparation / "cargo"
    private_home.mkdir()
    cache = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")).resolve()
    caches = [cache / name for name in ("registry", "git")]
    for path in caches:
        (private_home / path.name).symlink_to(path, target_is_directory=True)
    sb = preparation / "vendor.sb"
    sb.write_text(profile([*read_trees, *caches], preparation, literals))
    (output / "vendor.sb").write_bytes(sb.read_bytes())
    prep_env = dict(env, CARGO_HOME=str(private_home), HOME=str(preparation), TMPDIR=str(preparation))
    checked_run([cargo, "vendor", "--locked", "--offline", "--versioned-dirs", "--manifest-path",
                 src / "Cargo.toml", vendor], "vendor", preparation, prep_env, output, sb)
    packages = tomllib.loads((src / "Cargo.lock").read_text())["package"]
    expected = {p["name"] + "-" + p["version"]: p for p in packages if "source" in p}
    require(set(expected) == {p.name for p in vendor.iterdir()}, "unexpected vendored dependency set")
    inventory = {}
    for name, package in expected.items():
        checksums = (vendor / name / ".cargo-checksum.json").read_bytes()
        require(json.loads(checksums)["package"] == package.get("checksum"), f"changed package pin: {name}")
        inventory[name] = {"source": package["source"], "packageChecksum": package.get("checksum"),
                           "vendorChecksumsSha256": sha(checksums)}
    save(output / "locked-packages.json", inventory)


def verify(candidate, manifest_path, output):
    require((platform.system(), platform.machine()) == ("Darwin", "arm64"),
            "build-closure evidence currently requires native Apple Silicon macOS")
    manifest = tomllib.loads(manifest_path.read_text())
    require(manifest["features"] == [], "build-closure check currently covers default features")
    require(manifest["target"] == TRIPLE, "manifest target differs from the native sandbox")
    output.mkdir(parents=True, exist_ok=True)
    require(not any(output.iterdir()), "expected empty output")
    cargo = Path(capture(["rustup", "which", "--toolchain", "1.97.1", "cargo"])).resolve()
    toolchain = cargo.parents[1]
    rustc = toolchain / "bin/rustc"
    compiler = capture([rustc, "-vV"])
    require("commit-hash: 8bab26f4f68e0e26f0bb7960be334d5b520ea452" in compiler, "wrong rustc")
    sdk = Path(capture(["xcrun", "--show-sdk-path"])).resolve()
    clang = Path(capture(["xcrun", "--find", "clang"])).resolve()
    apple_tools = clang.parents[2]
    developer_bin = Path(capture(["xcrun", "--find", "make"])).resolve().parent
    with tempfile.TemporaryDirectory(prefix="oddlang-build-", dir="/private/tmp") as scratch:
        work = Path(scratch)
        src, vendor, writable = work / "candidate", work / "preparation/vendor", work / "scratch"
        src.mkdir()
        entries = stage_candidate(candidate, manifest, src)
        writable.mkdir()
        home = writable / "home"
        cargo_home = writable / "cargo"
        tmp = writable / "tmp"
        for folder in (home, cargo_home, tmp):
            folder.mkdir()
        target = writable / "target"
        env = {"HOME": str(home), "CARGO_HOME": str(cargo_home), "CARGO_TARGET_DIR": str(target),
               "CARGO_NET_OFFLINE": "true", "RUSTC": str(rustc), "RUSTDOC": str(toolchain / "bin/rustdoc"),
               "PATH": f"{toolchain}/bin:{clang.parent}:{developer_bin}:/usr/bin:/bin", "TMPDIR": str(tmp),
               "SDKROOT": str(sdk), "CC": str(clang), "CXX": str(clang.parent / "clang++"),
               "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": str(clang),
               "OPENSSL_CONF": "/dev/null", "MARMISH_BUILD_COMMIT": PIN, "LC_ALL": "C", "TZ": "UTC"}
        os_read = [Path(p) for p in ("/System/Library", "/usr/lib",
                   "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
                   "/System/Volumes/Preboot/Cryptexes/OS/usr/lib")]
        sys_bins = ["/bin/" + tool for tool in ("sh", "bash", "cat", "cp", "echo", "mkdir", "mv", "rm", "ln")]
        sys_bins += ["/usr/bin/" + tool for tool in
                     ("env", "git", "perl", "perl5.34", "awk", "sed", "grep", "tr", "touch", "uname",
                      "dirname", "basename", "find", "xargs", "cmp", "sort", "expr", "head", "tail")]
        sys_bins += [str(developer_bin / tool) for tool in ("make", "git")]
        literals = ["/", "/private/etc/ssl/openssl.cnf", "/dev/null", "/dev/zero", "/dev/urandom", "/dev/random", *sys_bins]
        trees = [src, vendor, toolchain, sdk, apple_tools, developer_bin.parent / "lib", *os_read]
        prepare_vendor(cargo, src, vendor, env, output, trees, literals)
        (cargo_home / "config.toml").write_bytes((output / "vendor.stdout").read_bytes())
        sb = work / "build.sb"
        sb.write_text(profile(trees, writable, literals))
        (output / "build.sb").write_bytes(sb.read_bytes())
        checked_run([cargo, "check", "--locked", "--offline", "--target", TRIPLE, "--lib",
                     "-p", "marmish-core", "-p", "marmish-native"], "cargo-check", src, env, output, sb)
        records, seen = check_depinfo(src, target, vendor, toolchain, sdk, entries)
        save(output / "dep-info.json", records)
        outputs = list(target.glob(f"{TRIPLE}/debug/build/marmish-native-*/output"))
        require(len(outputs) == 1 and
                f"cargo:rustc-env=MARMISH_BUILD_COMMIT={PIN}" in outputs[0].read_text(),
                "missing retained native build script execution evidence")
        (output / "native-build-script.txt").write_bytes(outputs[0].read_bytes())

        # Use an ordinary private copy to poison one declared build script. Its
        # readable canary deliberately sits beside, outside, every readable root.
        canary = work / "undeclared-input"
        canary.write_bytes(b"oddlang-build-boundary-canary")
        require(canary.read_bytes() == b"oddlang-build-boundary-canary", "invalid test control")
        script = src / NATIVE_SCRIPT
        text = script.read_text()
        require(text.count("fn main() {") == 1, "cannot instrument native build script")
        injected = """
    let e = std::fs::read(R"PATH").expect_err("EXTERNAL_CANARY_READ");
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "wrong canary failure");
    eprintln!("ODDLANG_UNDECLARED_READ_DENIED");
    std::process::exit(73);
""".replace('R"PATH"', json.dumps(str(canary)))
        script.write_text(text.replace("fn main() {", "fn main() {" + injected))
        rc = checked_run([cargo, "check", "--locked", "--offline", "--target", TRIPLE, "--lib",
                          "-p", "marmish-native"], "negative", src, env, output, sb, False)
        failure = (output / "negative.stderr").read_text()
        require("exit status: 73" in failure and
                re.search(r"(?m)^  ODDLANG_UNDECLARED_READ_DENIED$", failure) and
                "EXTERNAL_CANARY_READ" not in failure,
                "negative control failed without observing OS permission denial")
        save(output / "report.json", {"version": 1, "target": TRIPLE, "compiler": compiler,
             "scope": "Cargo check of assembled marmish-core and marmish-native libraries with default features",
             "manifestSha256": sha(manifest_path.read_bytes()), "cargoLockSha256": manifest["cargoLockSha256"],
             "sandboxSha256": sha(sb.read_bytes()), "vendorSandboxSha256": sha((output / "vendor.sb").read_bytes()),
             "lockedPackagesSha256": sha((output / "locked-packages.json").read_bytes()), "depInfoSha256": sha((output / "dep-info.json").read_bytes()),
             "generatedModule": {"path": FRAME, "sha256": entries[FRAME]["sha256"]},
             "nativeBuildScript": {"path": NATIVE_SCRIPT, "sha256": entries[NATIVE_SCRIPT]["sha256"],
                                   "stdoutSha256": sha((output / "native-build-script.txt").read_bytes())},
             "compilerDepfiles": len(records), "assembledCompilerInputs": sorted(seen),
             "negative": {"status": "permission denied", "cargoExitCode": rc, "buildScriptExitCode": 73,
                          "stderrSha256": sha((output / "negative.stderr").read_bytes())},
             "readTrees": list(map(str, trees)), "readFiles": literals, "scratch": str(writable),
             "limits": ["Compiler dep-info lists declared compiler prerequisites.",
                        "Direct dynamic filesystem reads are bounded by the sandbox; successful reads are not traced.",
                        "Metadata queries are permitted. The run uses an empty home and private writable scratch."]})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("assembled", "manifest", "empty_output"):
        parser.add_argument(arg, type=Path)
    args = parser.parse_args()
    try:
        verify(args.assembled.resolve(), args.manifest.resolve(), args.empty_output.resolve())
    except (ValueError, KeyError, OSError, subprocess.SubprocessError) as error:
        sys.exit(f"build-closure verification failed: {error}")
