//! Verify a pinned Marmish compile in two isolated macOS sandbox profiles.
use provider_validation::{Result, read, sha};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{Read, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

const FRAME: &str = "crates/marmish-core/src/frame.rs";
const SCRIPT: &str = "crates/marmish-native/build.rs";
const PIN: &str = "49fc705408a230bd0ad896248d08dbdd2faad05a";
const TRIPLE: &str = "aarch64-apple-darwin";
type Env = BTreeMap<String, String>;
type Entries = BTreeMap<String, Value>;
fn require(ok: bool, msg: impl Into<String>) -> Result {
    if ok { Ok(()) } else { Err(msg.into().into()) }
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn save(path: &Path, value: &Value) -> Result {
    let mut f = fs::File::create(path)?;
    serde_json::to_writer_pretty(&mut f, value)?;
    Ok(f.write_all(b"\n")?)
}
fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
fn capture(command: &mut Command) -> Result<String> {
    let got = command.output()?;
    require(got.status.success(), String::from_utf8_lossy(&got.stderr))?;
    Ok(String::from_utf8(got.stdout)?.trim().into())
}
fn files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(dir: &Path, all: &mut Vec<PathBuf>) -> Result {
        for entry in fs::read_dir(dir)? {
            let e = entry?;
            if e.file_type()?.is_dir() {
                visit(&e.path(), all)?;
            } else {
                all.push(e.path());
            }
        }
        Ok(())
    }
    let mut all = Vec::new();
    visit(root, &mut all)?;
    all.sort();
    Ok(all)
}
fn stage(src: &Path, manifest: &Value, dest: &Path) -> Result<Entries> {
    let mut entries = Entries::new();
    for row in manifest["files"].as_array().ok_or("missing files")? {
        let rel = row["path"].as_str().ok_or("missing path")?;
        require(
            !rel.is_empty()
                && rel.split('/').all(|p| !matches!(p, "" | "." | ".."))
                && entries.insert(rel.into(), row.clone()).is_none(),
            format!("invalid path {rel}"),
        )?;
    }
    require(
        entries
            .iter()
            .filter(|(_, v)| v["producer"] == "generated")
            .map(|(k, _)| k.as_str())
            .eq([FRAME]),
        "expected the generated framing module",
    )?;
    require(
        entries
            .get(SCRIPT)
            .is_some_and(|v| v["producer"] == "retained"),
        "native build script must be retained",
    )?;
    require(
        entries
            .get("Cargo.lock")
            .is_some_and(|v| v["sha256"] == manifest["cargoLockSha256"]),
        "wrong workspace lock",
    )?;
    let mut count = 0;
    for p in files(src)? {
        let rel = p.strip_prefix(src)?.to_str().ok_or("invalid filename")?;
        let meta = p.symlink_metadata()?;
        require(
            meta.is_file() && entries.contains_key(rel),
            format!("undeclared or special input: {rel}"),
        )?;
        let row = &entries[rel];
        let mode = meta.permissions().mode() & 0o777;
        let data = fs::read(&p)?;
        let expected = row["mode"]
            .as_str()
            .and_then(|s| s.get(s.len().checked_sub(3)?..))
            .and_then(|s| u32::from_str_radix(s, 8).ok());
        require(
            row["sha256"] == digest(&data) && expected == Some(mode),
            format!("assembled bytes or mode differ: {rel}"),
        )?;
        let out = dest.join(rel);
        fs::create_dir_all(out.parent().unwrap())?;
        fs::write(&out, data)?;
        fs::set_permissions(out, fs::Permissions::from_mode(mode))?;
        count += 1;
    }
    require(
        count == entries.len(),
        "assembled tree is missing manifest inputs",
    )?;
    Ok(entries)
}
fn filters(kind: &str, paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("({kind} {})", json!(p)))
        .collect::<Vec<_>>()
        .join(" ")
}
fn profile(trees: &[PathBuf], writable: &Path, literals: &[PathBuf]) -> String {
    let mut executable = trees.to_vec();
    executable.push(writable.to_owned());
    format!(
        "(version 1)\n(deny default)\n(allow file-read-metadata process-fork process-exec sysctl-read signal)\n(allow process-info* (target self))\n(allow file-map-executable {} {})\n(deny network*)\n(deny file-read-data file-write*)\n(allow file-write* (literal \"/dev/null\"))\n(allow file-read-data {} {})\n(allow file-read-data file-write* (subpath {}) (literal \"/dev/null\"))\n(deny mach-lookup (global-name-prefix \"com.apple.securityd\") (global-name \"com.apple.SecurityServer\"))\n",
        filters("subpath", &executable),
        filters("literal", literals),
        filters("subpath", trees),
        filters("literal", literals),
        json!(writable)
    )
}
fn dep_paths(text: &str) -> Result<Vec<String>> {
    let joined = text.replace("\\\n", "");
    let inputs = joined
        .lines()
        .next()
        .and_then(|line| line.split_once(": "))
        .ok_or("invalid Make dep-info")?
        .1;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut iter = inputs.chars();
    while let Some(ch) = iter.next() {
        if ch == '\\' {
            if let Some(next) = iter.next() {
                word.push(next);
            }
        } else if ch.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(ch);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}
fn resolve(p: &Path) -> Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for part in p.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => (),
            _ => {
                resolved.push(part.as_os_str());
                if resolved.is_symlink() {
                    resolved = resolved.canonicalize()?;
                }
            }
        }
    }
    Ok(resolved)
}
fn depinfo(
    src: &Path,
    target: &Path,
    vendor: &Path,
    toolchain: &Path,
    sdk: &Path,
    entries: &Entries,
) -> Result<(Vec<Value>, BTreeSet<String>)> {
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    for dep in files(target)? {
        let rel = dep.strip_prefix(target)?;
        if dep.extension().is_none_or(|x| x != "d")
            || rel.components().any(|c| c.as_os_str() == "out")
        {
            continue;
        }
        let mut deps = Vec::new();
        for name in dep_paths(&fs::read_to_string(&dep)?)? {
            let p = resolve(&src.join(name))?;
            let (label, boundary) = [
                ("assembled", src),
                ("locked dependency", vendor),
                ("build output", target),
                ("toolchain", toolchain),
                ("SDK", sdk),
            ]
            .into_iter()
            .find(|(_, root)| p.starts_with(root))
            .ok_or_else(|| format!("compiler dep-info escaped declared roots: {}", p.display()))?;
            let name = path(p.strip_prefix(boundary)?);
            if label == "assembled" {
                require(
                    entries.contains_key(&name),
                    format!("undeclared compiler input {name}"),
                )?;
                seen.insert(name.clone());
            }
            deps.push(json!([label, name]));
        }
        records.push(json!({"file":rel,"inputs":deps}));
    }
    require(
        seen.contains(FRAME),
        "compiler did not report generated frame.rs",
    )?;
    require(
        seen.contains(SCRIPT),
        "compiler did not report retained native build.rs",
    )?;
    Ok((records, seen))
}
fn checked(
    command: &[String],
    name: &str,
    dir: &Path,
    env: &Env,
    out: &Path,
    sb: Option<&Path>,
    success: bool,
) -> Result<i32> {
    let (first, rest) = command.split_first().ok_or("empty command")?;
    let mut cmd = if let Some(profile) = sb {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-f").arg(profile).arg(first);
        c
    } else {
        Command::new(first)
    };
    let status = cmd
        .args(rest)
        .current_dir(dir)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(fs::File::create(out.join(format!("{name}.stdout")))?)
        .stderr(fs::File::create(out.join(format!("{name}.stderr")))?)
        .status()?;
    let rc = status
        .code()
        .ok_or_else(|| format!("{name} killed by signal"))?;
    require(
        (rc == 0) == success,
        format!("{name} exited {rc}; see {}/{name}.stderr", out.display()),
    )?;
    Ok(rc)
}
fn vendor(
    cargo: &Path,
    src: &Path,
    dest: &Path,
    env: &Env,
    out: &Path,
    trees: &[PathBuf],
    literals: &[PathBuf],
) -> Result {
    let prep = dest.parent().ok_or("vendor parent")?;
    fs::create_dir(prep)?;
    let home = prep.join("cargo");
    fs::create_dir(&home)?;
    let cache = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap()).join(".cargo"))
        .canonicalize()?;
    let mut roots = trees.to_vec();
    for name in ["registry", "git"] {
        let dir = cache.join(name);
        symlink(&dir, home.join(name))?;
        roots.push(dir);
    }
    let sb = prep.join("vendor.sb");
    fs::write(&sb, profile(&roots, prep, literals))?;
    fs::copy(&sb, out.join("vendor.sb"))?;
    let mut env = env.clone();
    env.extend([
        ("HOME".into(), path(prep)),
        ("CARGO_HOME".into(), path(&home)),
        ("TMPDIR".into(), path(prep)),
    ]);
    checked(
        &[
            path(cargo),
            "vendor".into(),
            "--locked".into(),
            "--offline".into(),
            "--versioned-dirs".into(),
            "--manifest-path".into(),
            path(&src.join("Cargo.toml")),
            path(dest),
        ],
        "vendor",
        prep,
        &env,
        out,
        Some(&sb),
        true,
    )?;
    let lock: Value = toml::from_str(&fs::read_to_string(src.join("Cargo.lock"))?)?;
    let mut inventory = json!({});
    for p in lock["package"]
        .as_array()
        .ok_or("missing lock packages")?
        .iter()
        .filter(|v| v.get("source").is_some())
    {
        let name = format!(
            "{}-{}",
            p["name"].as_str().ok_or("package name")?,
            p["version"].as_str().ok_or("package version")?
        );
        let checksums = fs::read(dest.join(&name).join(".cargo-checksum.json"))?;
        let checks: Value = serde_json::from_slice(&checksums)?;
        require(
            checks.get("package") == Some(&p["checksum"]),
            format!("changed package pin: {name}"),
        )?;
        inventory[name] = json!({"source":p["source"],"packageChecksum":p["checksum"],"vendorChecksumsSha256":digest(&checksums)});
    }
    let actual = fs::read_dir(dest)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    require(
        actual.iter().eq(inventory.as_object().unwrap().keys()),
        "unexpected vendored dependency set",
    )?;
    save(&out.join("locked-packages.json"), &inventory)
}
fn negative(log: &str) -> Result {
    require(
        log.contains("exit status: 73")
            && log.lines().any(|s| s == "  ODDLANG_UNDECLARED_READ_DENIED")
            && !log.contains("EXTERNAL_CANARY_READ"),
        "negative control failed without observing OS permission denial",
    )
}
fn verify(candidate: &Path, manifest_path: &Path, out: &Path) -> Result {
    require(
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
        "build-closure evidence currently requires native Apple Silicon macOS",
    )?;
    let manifest: Value = toml::from_str(&fs::read_to_string(manifest_path)?)?;
    require(
        manifest["features"] == json!([]) && manifest["target"] == TRIPLE,
        "expected default features on native target",
    )?;
    fs::create_dir_all(out)?;
    require(fs::read_dir(out)?.next().is_none(), "expected empty output")?;
    let cargo = PathBuf::from(capture(Command::new("rustup").args([
        "which",
        "--toolchain",
        "1.97.1",
        "cargo",
    ]))?)
    .canonicalize()?;
    let toolchain = cargo.ancestors().nth(2).unwrap();
    let rustc = toolchain.join("bin/rustc");
    let compiler = capture(Command::new(&rustc).arg("-vV"))?;
    require(
        compiler.contains("commit-hash: 8bab26f4f68e0e26f0bb7960be334d5b520ea452"),
        "wrong rustc",
    )?;
    let xcrun = |args: &[&str]| -> Result<PathBuf> {
        Ok(PathBuf::from(capture(Command::new("xcrun").args(args))?).canonicalize()?)
    };
    let sdk = xcrun(&["--show-sdk-path"])?;
    let clang = xcrun(&["--find", "clang"])?;
    let make = xcrun(&["--find", "make"])?;
    let developer = make.parent().unwrap();
    let clang_bin = clang.parent().unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("oddlang-build-")
        .tempdir_in("/private/tmp")?;
    let work = scratch.path();
    let src = work.join("candidate");
    let vendor_dir = work.join("preparation/vendor");
    let writable = work.join("scratch");
    fs::create_dir(&src)?;
    let entries = stage(candidate, &manifest, &src)?;
    for dir in ["home", "cargo", "tmp"] {
        fs::create_dir_all(writable.join(dir))?;
    }
    let target = writable.join("target");
    let mut env: Env = [
        ("HOME", path(&writable.join("home"))),
        ("CARGO_HOME", path(&writable.join("cargo"))),
        ("CARGO_TARGET_DIR", path(&target)),
        ("CARGO_NET_OFFLINE", "true".into()),
        ("RUSTC", path(&rustc)),
        ("RUSTDOC", path(&toolchain.join("bin/rustdoc"))),
        (
            "PATH",
            format!(
                "{}/bin:{}:{}:/usr/bin:/bin",
                toolchain.display(),
                clang_bin.display(),
                developer.display()
            ),
        ),
        ("TMPDIR", path(&writable.join("tmp"))),
        ("SDKROOT", path(&sdk)),
        ("CC", path(&clang)),
        ("CXX", path(&clang_bin.join("clang++"))),
        ("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", path(&clang)),
        ("OPENSSL_CONF", "/dev/null".into()),
        ("MARMISH_BUILD_COMMIT", PIN.into()),
        ("LC_ALL", "C".into()),
        ("TZ", "UTC".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    if let Some(selection) = env::var_os("DEVELOPER_DIR") {
        env.insert(
            "DEVELOPER_DIR".into(),
            path(&PathBuf::from(selection).canonicalize()?),
        );
    }
    let mut trees = vec![
        src.clone(),
        vendor_dir.clone(),
        toolchain.into(),
        sdk.clone(),
        clang.ancestors().nth(3).unwrap().into(),
        developer.parent().unwrap().join("lib"),
    ];
    trees.extend(
        [
            "/System/Library",
            "/usr/lib",
            "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
            "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
        ]
        .map(PathBuf::from),
    );
    let mut literal = [
        "/",
        "/private/etc/ssl/openssl.cnf",
        "/dev/null",
        "/dev/zero",
        "/dev/urandom",
        "/dev/random",
    ]
    .map(PathBuf::from)
    .to_vec();
    literal.extend(
        ["sh", "bash", "cat", "cp", "echo", "mkdir", "mv", "rm", "ln"]
            .map(|name| Path::new("/bin").join(name)),
    );
    literal.extend(
        [
            "env", "git", "perl", "perl5.34", "awk", "sed", "grep", "tr", "touch", "uname",
            "dirname", "basename", "find", "xargs", "cmp", "sort", "expr", "head", "tail",
        ]
        .map(|name| Path::new("/usr/bin").join(name)),
    );
    literal.extend(["make", "git"].map(|name| developer.join(name)));
    vendor(&cargo, &src, &vendor_dir, &env, out, &trees, &literal)?;
    fs::copy(
        out.join("vendor.stdout"),
        writable.join("cargo/config.toml"),
    )?;
    let sb = work.join("build.sb");
    fs::write(&sb, profile(&trees, &writable, &literal))?;
    fs::copy(&sb, out.join("build.sb"))?;
    let mut args = vec![path(&cargo)];
    args.extend(
        [
            "check",
            "--locked",
            "--offline",
            "--target",
            TRIPLE,
            "--lib",
            "-p",
            "marmish-core",
            "-p",
            "marmish-native",
        ]
        .map(String::from),
    );
    checked(&args, "cargo-check", &src, &env, out, Some(&sb), true)?;
    let (records, seen) = depinfo(&src, &target, &vendor_dir, toolchain, &sdk, &entries)?;
    save(&out.join("dep-info.json"), &json!(records))?;
    let build = target.join(TRIPLE).join("debug/build");
    let mut outputs = Vec::new();
    for entry in fs::read_dir(build)? {
        let entry = entry?;
        let p = entry.path().join("output");
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("marmish-native-")
            && p.is_file()
        {
            outputs.push(p);
        }
    }
    require(
        outputs.len() == 1
            && fs::read_to_string(&outputs[0])?
                .contains(&format!("cargo:rustc-env=MARMISH_BUILD_COMMIT={PIN}")),
        "missing retained native build script execution evidence",
    )?;
    fs::copy(&outputs[0], out.join("native-build-script.txt"))?;
    let canary = work.join("undeclared-input");
    fs::write(&canary, b"oddlang-build-boundary-canary")?;
    require(
        fs::read(&canary)? == b"oddlang-build-boundary-canary",
        "invalid test control",
    )?;
    let script = src.join(SCRIPT);
    let original = fs::read_to_string(&script)?;
    require(
        original.matches("fn main() {").count() == 1,
        "cannot instrument native build script",
    )?;
    let inject = format!(
        r#"
    let e = std::fs::read({}).expect_err("EXTERNAL_CANARY_READ");
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "wrong canary failure");
    eprintln!("ODDLANG_UNDECLARED_READ_DENIED");
    std::process::exit(73);
"#,
        json!(&canary)
    );
    fs::write(
        script,
        original.replace("fn main() {", &format!("fn main() {{{inject}")),
    )?;
    args.drain(7..9);
    let rc = checked(&args, "negative", &src, &env, out, Some(&sb), false)?;
    negative(&fs::read_to_string(out.join("negative.stderr"))?)?;
    save(
        &out.join("report.json"),
        &json!({
        "version":1,"target":TRIPLE,"compiler":compiler,
        "scope":"Cargo check of assembled marmish-core and marmish-native libraries with default features",
        "manifestSha256":sha(manifest_path)?,"cargoLockSha256":manifest["cargoLockSha256"],
        "sandboxSha256":sha(&sb)?,"vendorSandboxSha256":sha(&out.join("vendor.sb"))?,
        "lockedPackagesSha256":sha(&out.join("locked-packages.json"))?,"depInfoSha256":sha(&out.join("dep-info.json"))?,
        "generatedModule":{"path":FRAME,"sha256":entries[FRAME]["sha256"]},
        "nativeBuildScript":{"path":SCRIPT,"sha256":entries[SCRIPT]["sha256"],"stdoutSha256":sha(&out.join("native-build-script.txt"))?},
        "compilerDepfiles":records.len(),"assembledCompilerInputs":seen,
        "negative":{"status":"permission denied","cargoExitCode":rc,"buildScriptExitCode":73,"stderrSha256":sha(&out.join("negative.stderr"))?},
        "readTrees":trees,"readFiles":literal,"scratch":writable,
        "limits":["Compiler dep-info lists declared compiler prerequisites.","Direct dynamic filesystem reads are bounded by the sandbox; successful reads are not traced.",
        "Metadata queries are permitted. The run uses an empty home and private writable scratch."]}),
    )
}
fn replay(manifest_path: &Path, out: &Path) -> Result {
    let report = read(&out.join("report.json"))?;
    let manifest: Value = toml::from_str(&fs::read_to_string(manifest_path)?)?;
    require(
        report["version"] == 1
            && report["target"] == TRIPLE
            && report["manifestSha256"] == sha(manifest_path)?
            && report["cargoLockSha256"] == manifest["cargoLockSha256"],
        "wrong build closure identity",
    )?;
    for (key, file) in [
        ("sandboxSha256", "build.sb"),
        ("vendorSandboxSha256", "vendor.sb"),
        ("lockedPackagesSha256", "locked-packages.json"),
        ("depInfoSha256", "dep-info.json"),
    ] {
        require(
            report[key] == sha(&out.join(file))?,
            format!("stale {file}"),
        )?;
    }
    let trees: Vec<PathBuf> = serde_json::from_value(report["readTrees"].clone())?;
    let literals: Vec<PathBuf> = serde_json::from_value(report["readFiles"].clone())?;
    let writable: PathBuf = serde_json::from_value(report["scratch"].clone())?;
    require(
        profile(&trees, &writable, &literals) == fs::read_to_string(out.join("build.sb"))?,
        "build profile differs from recorded boundaries",
    )?;
    let dep = read(&out.join("dep-info.json"))?;
    let records = dep.as_array().ok_or("missing dep-info")?;
    let mut seen = BTreeSet::new();
    for record in records {
        for row in record["inputs"].as_array().ok_or("missing prerequisites")? {
            if row[0] == "assembled" {
                seen.insert(row[1].as_str().ok_or("expected prerequisite")?);
            }
        }
    }
    require(
        report["compilerDepfiles"] == json!(records.len())
            && seen.contains(FRAME)
            && seen.contains(SCRIPT)
            && report["assembledCompilerInputs"] == json!(seen),
        "incorrect compiler closure",
    )?;
    for (field, rel) in [("generatedModule", FRAME), ("nativeBuildScript", SCRIPT)] {
        let entry = manifest["files"]
            .as_array()
            .ok_or("missing files")?
            .iter()
            .find(|r| r["path"] == rel)
            .ok_or("missing manifest entry")?;
        require(
            report[field]["path"] == rel && report[field]["sha256"] == entry["sha256"],
            "incorrect source digest",
        )?;
    }
    require(
        report["nativeBuildScript"]["stdoutSha256"] == sha(&out.join("native-build-script.txt"))?
            && fs::read_to_string(out.join("native-build-script.txt"))?
                .contains(&format!("cargo:rustc-env=MARMISH_BUILD_COMMIT={PIN}")),
        "missing retained script observation",
    )?;
    require(
        report["negative"]["status"] == "permission denied"
            && report["negative"]["buildScriptExitCode"] == 73
            && report["negative"]["cargoExitCode"]
                .as_i64()
                .is_some_and(|v| v != 0)
            && report["negative"]["stderrSha256"] == sha(&out.join("negative.stderr"))?,
        "stale negative control",
    )?;
    negative(&fs::read_to_string(out.join("negative.stderr"))?)
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Request {
    Profile {
        trees: Vec<PathBuf>,
        writable: PathBuf,
        literals: Vec<PathBuf>,
    },
    Vendor {
        cargo: PathBuf,
        src: PathBuf,
        dest: PathBuf,
        env: Env,
        out: PathBuf,
        trees: Vec<PathBuf>,
        literals: Vec<PathBuf>,
    },
    Run {
        command: Vec<String>,
        name: String,
        dir: PathBuf,
        env: Env,
        out: PathBuf,
        sandbox: Option<PathBuf>,
        success: bool,
    },
}
fn main() -> Result {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [request] if request == "request" => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            let result = match serde_json::from_slice(&bytes)? {
                Request::Profile {
                    trees,
                    writable,
                    literals,
                } => json!(profile(&trees, &writable, &literals)),
                Request::Vendor {
                    cargo,
                    src,
                    dest,
                    env,
                    out,
                    trees,
                    literals,
                } => {
                    vendor(&cargo, &src, &dest, &env, &out, &trees, &literals)?;
                    Value::Null
                }
                Request::Run {
                    command,
                    name,
                    dir,
                    env,
                    out,
                    sandbox,
                    success,
                } => json!(checked(
                    &command,
                    &name,
                    &dir,
                    &env,
                    &out,
                    sandbox.as_deref(),
                    success
                )?),
            };
            println!("{}", result);
            Ok(())
        }
        [mode, manifest, out] if mode == "replay" => replay(manifest.as_ref(), out.as_ref()),
        [src, manifest, out] => verify(src.as_ref(), manifest.as_ref(), out.as_ref()),
        _ => Err(
            "expected ASSEMBLED MANIFEST EMPTY_OUTPUT or replay MANIFEST ARCHIVED_OUTPUT".into(),
        ),
    }
}
#[cfg(test)]
#[path = "build_closure/tests.rs"]
mod tests;
