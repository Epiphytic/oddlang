//! Qualify the regenerated stream crate in a source-free, isolated checkout.
#[path = "stream-workspace/consumers.rs"]
mod consumers;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use marmish_inventory::{
    Map, PIN, Set, keys, object, read, read_json, stream_baseline,
    stream_workspace::{
        Environment, Tools, capture, closure, denied_reads, executable, git_archive, profile, save,
    },
    strings, text, valid_hash,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};
const PACKAGE: &str = "crates/marmish-stream";
#[derive(Parser)]
struct Args {
    #[arg(long)]
    repository: PathBuf,
    #[arg(long)]
    oddlang: PathBuf,
    #[arg(long)]
    original: PathBuf,
    #[arg(long)]
    baseline: PathBuf,
    #[arg(long)]
    oracle_original: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    commit: String,
    #[arg(long)]
    bundle: String,
    #[arg(long)]
    bundle_sha256: String,
    #[arg(long, required = true)]
    deny_read: Vec<PathBuf>,
}
fn file_sha(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut hash = Sha256::new();
    io::copy(&mut f, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}
fn checked(
    helper: &Path,
    command: Value,
    name: &str,
    dir: &Path,
    env: &Environment,
    out: &Path,
    sandbox: &Path,
) -> Result<()> {
    closure(
        helper,
        json!({"op":"run","command":command,"name":name,"dir":dir,"out":out,"env":env,"sandbox":sandbox,"success":true}),
    )?;
    Ok(())
}
fn files(root: &Path) -> Result<Set> {
    fn visit(root: &Path, dir: &Path, paths: &mut Set) -> Result<()> {
        for e in fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            let ty = e.file_type()?;
            if ty.is_dir() {
                visit(root, &p, paths)?
            } else {
                ensure!(ty.is_file(), "not a regular input {}", p.display());
                paths.insert(
                    p.strip_prefix(root)?
                        .to_str()
                        .context("UTF-8 file path")?
                        .into(),
                );
            }
        }
        Ok(())
    }
    let mut paths = Set::new();
    visit(root, root, &mut paths)?;
    Ok(paths)
}
fn object_sample(repo: &Path, relative: &str) -> Result<PathBuf> {
    let blob = capture(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", &format!("HEAD:{relative}")]),
    )?;
    let common = PathBuf::from(capture(Command::new("git").arg("-C").arg(repo).args([
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]))?);
    let objects = common.join("objects");
    let loose = objects.join(&blob[..2]).join(&blob[2..]);
    if loose.is_file() {
        return Ok(loose);
    }
    let mut packs = vec![];
    for e in fs::read_dir(objects.join("pack"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "pack") {
            packs.push(p)
        }
    }
    packs.sort();
    packs.into_iter().next().context("missing Git object data")
}
fn identity(tools: &Tools) -> Result<Value> {
    let linker =
        Path::new(&capture(Command::new("xcrun").args(["--find", "ld"]))?).canonicalize()?;
    Ok(
        json!({"rustc":capture(Command::new(tools.chain.join("bin/rustc")).arg("-vV"))?,"clang":{"path":tools.clang,"sha256":file_sha(&tools.clang)?,"version":capture(Command::new(&tools.clang).arg("--version"))?},"linker":{"path":linker,"sha256":file_sha(&linker)?},"sdk":{"path":tools.sdk,"version":capture(Command::new("xcrun").arg("--show-sdk-version"))?,"settingsSha256":file_sha(&tools.sdk.join("SDKSettings.json"))?},"developerBin":tools.developer,"macOS":capture(Command::new("sw_vers").arg("-productVersion"))?}),
    )
}
fn extended(a: &[PathBuf], b: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut v = a.to_owned();
    v.extend(b);
    v
}
fn oracle(
    exe: &Path,
    root: &Path,
    writable: &Path,
    env: &Environment,
    sandbox: &Path,
    tools: &Tools,
    role: &str,
    peer: &Path,
    out: &Path,
    baseline: &Path,
    commit: &str,
    archive: &str,
    bundle: Option<&str>,
) -> Result<()> {
    let mut c = Command::new(exe);
    c.arg("--workspace")
        .arg(root)
        .arg("--target-dir")
        .arg(writable.join("oracle-target"))
        .arg("--receipt")
        .arg(out)
        .args(["--role", role, "--toolchain"])
        .arg(&tools.chain)
        .arg("--sandbox")
        .arg(sandbox)
        .arg("--peer-receipt")
        .arg(peer)
        .arg("--baseline")
        .arg(baseline)
        .args(["--commit", commit, "--archive-sha256", archive]);
    if let Some(pin) = bundle {
        c.args(["--bundle-sha256", pin]);
    }
    ensure!(
        c.env_clear()
            .envs(env)
            .current_dir(root)
            .status()?
            .success(),
        "stream oracle failed"
    );
    Ok(())
}
fn collect(
    src: &Path,
    out: &Path,
    baseline: &Value,
    env: &Environment,
    build: &Path,
    vendor: &Path,
) -> Result<()> {
    fs::create_dir(out)?;
    let report = stream_baseline::capture(
        src,
        out,
        Some(baseline),
        Path::new(&env["CARGO_HOME"]),
        Some(build),
        Some(vendor),
    )?;
    stream_baseline::compare(baseline, &report)?;
    save(&out.join("receipt.json"), &report)
}
fn controls(
    src: &Path,
    writable: &Path,
    env: &Environment,
    build: &Path,
    vendor: &Path,
    cargo: &Path,
    baseline: &Value,
    out: &Path,
) -> Result<Value> {
    let mut e = env.clone();
    e.insert(
        "CARGO_TARGET_DIR".into(),
        writable.join("negative-target").display().to_string(),
    );
    let module = src.join(PACKAGE).join("src/copy_protocol.rs");
    let old = read(&module)?;
    fs::remove_file(&module)?;
    let code = consumers::run_capture(
        &[
            cargo.display().to_string(),
            "check".into(),
            "--locked".into(),
            "--offline".into(),
            "-p".into(),
            "marmish-stream".into(),
        ],
        "missing-module",
        src,
        &e,
        build,
        out,
    );
    fs::write(&module, old)?;
    ensure!(
        code? == 101
            && fs::read_to_string(out.join("missing-module.stderr"))?
                .contains("file not found for module `copy_protocol`"),
        "missing expected compile error for withdrawn copy_protocol.rs"
    );
    let marker = src.join(PACKAGE).join("src/undeclared-input");
    let lib = src.join(PACKAGE).join("src/lib.rs");
    let old = read(&lib)?;
    fs::write(&marker, b"oddlang-independent-undeclared-input")?;
    let result = (|| -> Result<()> {
        let mut new = old.clone();
        new.extend(b"\nconst _: &[u8] = include_bytes!(\"undeclared-input\");\n");
        fs::write(&lib, new)?;
        collect(
            src,
            &writable.join("negative-baseline"),
            baseline,
            &e,
            build,
            vendor,
        )
    })();
    fs::write(&lib, old)?;
    fs::remove_file(&marker)?;
    let expected = format!("undeclared compiler input {PACKAGE}/src/undeclared-input");
    ensure!(
        result.err().is_some_and(|e| e.to_string() == expected),
        "undeclared compiler input was not diagnosed"
    );
    Ok(json!({"missingModule":101,"undeclaredInput":expected}))
}
fn candidate_profile(
    helper: &Path,
    tools: &Tools,
    src: &Path,
    writable: &Path,
    bundle: &Path,
    vendor: &Path,
    build: &Path,
) -> Result<()> {
    let (trees, literals) = tools.boundaries();
    let policy = profile(
        helper,
        &extended(&trees, [src.to_owned(), vendor.to_owned()]),
        writable,
        &extended(&literals, writable.ancestors().skip(1).map(Path::to_owned)),
    )?;
    fs::write(
        build,
        policy
            + &format!(
                "(allow network* (subpath {}))\n(deny file-read-data (subpath {}))\n",
                json!(writable),
                json!(bundle)
            ),
    )?;
    Ok(())
}
fn qualify(mut args: Args) -> Result<()> {
    ensure!(
        cfg!(all(target_os = "macos", target_arch = "aarch64")),
        "qualification requires native Apple Silicon macOS"
    );
    args.repository = args.repository.canonicalize()?;
    args.original = args.original.canonicalize()?;
    args.baseline = args.baseline.canonicalize()?;
    args.oddlang = args.oddlang.canonicalize()?;
    args.oracle_original = args.oracle_original.canonicalize()?;
    fs::create_dir(&args.output)?;
    let out = args.output.canonicalize()?;
    ensure!(
        !out.starts_with(&args.repository) && !out.starts_with(&args.original),
        "evidence must be outside both checkouts"
    );
    let oddlang_pin = file_sha(&args.oddlang)?;
    let baseline = read_json(&args.baseline)?;
    let expected = object(&baseline["trackedSources"])?;
    ensure!(
        baseline["upstreamCommit"] == PIN && !expected.is_empty(),
        "wrong frozen stream baseline"
    );
    let original_oracle = read_json(&args.oracle_original)?;
    ensure!(
        original_oracle["upstream_commit"] == PIN
            && original_oracle["role"] == "original"
            && original_oracle["all_matched"] == true
            && original_oracle["workspace_identity"]["baselineSha256"] == file_sha(&args.baseline)?,
        "wrong original stream oracle receipt"
    );
    let src = out.join("candidate");
    let archive = git_archive(&args.repository, &args.commit, &src)?;
    for (name, pin) in object(&baseline["cargoInputs"])? {
        ensure!(
            file_sha(&src.join(name))? == *pin,
            "candidate changed baseline Cargo input {name}"
        );
    }
    ensure!(
        !files(&src.join(PACKAGE))?
            .iter()
            .any(|p| p.ends_with(".rs")),
        "downstream commit contains original Rust source in stream package"
    );
    ensure!(
        args.bundle
            .split('/')
            .all(|p| !matches!(p, "" | "." | "..")),
        "invalid bundle path"
    );
    let bundle = src.join(&args.bundle).canonicalize()?;
    ensure!(
        bundle.starts_with(&src) && bundle.is_dir(),
        "bundle directory outside checkout"
    );
    let inputs = ["bundle.lock", "sources.json", "syntax.ttl"].map(|p| bundle.join(p));
    for p in &inputs {
        ensure!(
            fs::symlink_metadata(p)?.is_file(),
            "missing bundle input {}",
            p.display()
        );
    }
    ensure!(
        valid_hash(&json!(args.bundle_sha256), 64) && file_sha(&inputs[0])? == args.bundle_sha256,
        "bundle lock digest mismatches pin"
    );
    let canary = out.join("outside-sandbox");
    fs::write(&canary, b"outside candidate and writable build roots\n")?;
    let first = expected.keys().next().unwrap();
    let mut denied = json!({"originalSource":args.original.join(first),"originalObjectStore":object_sample(&args.original,first)?,"candidateObjectStore":object_sample(&args.repository,"Cargo.toml")?,"unlistedInput":canary});
    for (i, path) in args.deny_read.iter().enumerate() {
        denied[format!("retainedArtifact{i}")] = json!(path.canonicalize()?);
    }
    let tools = Tools::discover()?;
    let native = identity(&tools)?;
    let cargo = tools.chain.join("bin/cargo");
    let (trees, literals) = tools.boundaries();
    let helper = executable("provider-validation", "build-closure")?;
    let driver = executable("marmish-inventory", "stream-driver")?;
    let a = out.join("a");
    let env = tools.environment(&a, &src)?;
    let emitter = out.join("emitter");
    fs::create_dir(&emitter)?;
    let emit = out.join("emit.sb");
    let mut ef = literals.clone();
    ef.push(args.oddlang.clone());
    ef.extend(inputs.clone());
    fs::write(&emit, profile(&helper, &trees, &emitter, &ef)?)?;
    let mut observed = json!({"emitter":denied_reads(&emit,denied.clone(),&emitter,&env)?});
    let generated = emitter.join("generated");
    checked(
        &helper,
        json!([
            args.oddlang,
            "build",
            "rust-tree",
            bundle,
            generated,
            "--bundle-sha256",
            args.bundle_sha256
        ]),
        "emit",
        &emitter,
        &env,
        &out,
        &emit,
    )?;
    let generated_names: Set = files(&generated)?
        .into_iter()
        .map(|p| format!("{PACKAGE}/{p}"))
        .collect();
    ensure!(
        generated_names == keys(&baseline["trackedSources"])?,
        "emitted tracked Rust paths differ"
    );
    for (name, row) in expected {
        ensure!(
            file_sha(&generated.join(Path::new(name).strip_prefix(PACKAGE)?))? == row["sha256"],
            "emitted Rust digest changed: {name}"
        );
    }
    let bootstrap = src.join("oddlang/regenerate-stream.sh");
    let bootstrap_pin = src.join("oddlang/stream.sha256");
    ensure!(
        fs::read_to_string(&bootstrap_pin)?.trim() == args.bundle_sha256,
        "bootstrap uses a different pin"
    );
    let boot = out.join("bootstrap.sb");
    let mut bf = extended(&literals, [src.clone(), src.join("oddlang")]);
    bf.extend(src.ancestors().skip(1).map(Path::to_owned));
    bf.extend([args.oddlang.clone(), bootstrap.clone(), bootstrap_pin]);
    bf.extend(inputs.clone());
    fs::write(&boot, profile(&helper, &trees, &src.join(PACKAGE), &bf)?)?;
    observed["bootstrap"] = denied_reads(&boot, denied.clone(), &src, &env)?;
    let mut boot_env = env.clone();
    boot_env.insert("ODDLANG".into(), args.oddlang.display().to_string());
    for name in ["bootstrap", "bootstrap-replay"] {
        checked(
            &helper,
            json!(["/bin/bash", bootstrap]),
            name,
            &src,
            &boot_env,
            &out,
            &boot,
        )?;
    }
    ensure!(
        file_sha(&args.oddlang)? == oddlang_pin,
        "Oddlang emitter changed during regeneration"
    );
    for (name, row) in expected {
        ensure!(
            file_sha(&src.join(name))? == row["sha256"],
            "bootstrap output differs: {name}"
        );
    }
    ensure!(
        files(&src.join(PACKAGE))?
            .into_iter()
            .filter(|p| p.ends_with(".rs"))
            .map(|p| format!("{PACKAGE}/{p}"))
            .collect::<Set>()
            == generated_names,
        "candidate Rust tree differs from emitted inventory"
    );
    let vendor = out.join("preparation/vendor");
    closure(
        &helper,
        json!({"op":"vendor","cargo":cargo,"src":src,"dest":vendor,"env":env,"out":out,"trees":extended(&trees,[src.clone()]),"literals":literals}),
    )?;
    let config = out.join("vendor.stdout");
    fs::copy(&config, a.join("cargo/config.toml"))?;
    let build = out.join("build.sb");
    candidate_profile(&helper, &tools, &src, &a, &bundle, &vendor, &build)?;
    denied["graph"] = json!(inputs[2]);
    observed["candidateBuild"] = denied_reads(&build, denied, &src, &env)?;
    save(&out.join("denied-reads.json"), &observed)?;
    collect(&src, &a.join("stream"), &baseline, &env, &build, &vendor)?;
    let consumers = consumers::run(
        &helper,
        &src,
        &a,
        &env,
        &build,
        &out.join("candidate-consumers"),
        &cargo,
        true,
    )?;
    save(&out.join("candidate-consumers.json"), &consumers)?;
    let cand_oracle = out.join("candidate-oracle.json");
    oracle(
        &driver,
        &src,
        &a,
        &env,
        &build,
        &tools,
        "candidate",
        &args.oracle_original,
        &cand_oracle,
        &args.baseline,
        &args.commit,
        &archive,
        Some(&args.bundle_sha256),
    )?;
    let negative = controls(&src, &a, &env, &build, &vendor, &cargo, &baseline, &out)?;
    let original = out.join("upstreams");
    let orig_archive = git_archive(&args.original, PIN, &original)?;
    let b = out.join("b");
    let orig_env = tools.environment(&b, &original)?;
    fs::copy(&config, b.join("cargo/config.toml"))?;
    let orig_profile = out.join("original.sb");
    fs::write(
        &orig_profile,
        profile(
            &helper,
            &extended(&trees, [original.clone(), vendor]),
            &b,
            &extended(&literals, b.ancestors().skip(1).map(Path::to_owned)),
        )? + &format!("(allow network* (subpath {}))\n", json!(b)),
    )?;
    let orig_cons = consumers::run(
        &helper,
        &original,
        &b,
        &orig_env,
        &orig_profile,
        &out.join("original-consumers"),
        &cargo,
        false,
    )?;
    save(&out.join("original-consumers.json"), &orig_cons)?;
    consumers::compare(&orig_cons, &consumers)?;
    oracle(
        &driver,
        &original,
        &b,
        &orig_env,
        &orig_profile,
        &tools,
        "original",
        &cand_oracle,
        &out.join("reverse-oracle.json"),
        &args.baseline,
        PIN,
        &orig_archive,
        None,
    )?;
    let mut evidence = Map::new();
    for name in [
        "emit.sb",
        "bootstrap.sb",
        "build.sb",
        "original.sb",
        "a/stream/receipt.json",
        "denied-reads.json",
        "locked-packages.json",
        "candidate-consumers.json",
        "original-consumers.json",
        "candidate-consumers/runtime.sb",
        "original-consumers/runtime.sb",
        "candidate-oracle.json",
        "reverse-oracle.json",
    ] {
        evidence.insert(name.into(), json!(file_sha(&out.join(name))?));
    }
    let receipt = out.join("qualification.json");
    save(
        &receipt,
        &json!({"version":1,"marmishOddlangCommit":args.commit,"archiveSha256":archive,"originalCommit":PIN,"originalArchiveSha256":orig_archive,"sourceArchiveRustPaths":[],"generatedPaths":generated_names,"bundleSha256":args.bundle_sha256,"syntaxGraphSha256":file_sha(&inputs[2])?,"oddlangSha256":oddlang_pin,"originalBaselineSha256":file_sha(&args.baseline)?,"nativeToolchain":native,"consumerScope":consumers::scope(),"negativeControls":negative,"evidenceSha256":evidence}),
    )?;
    println!("{}", receipt.display());
    Ok(())
}
fn main() -> Result<()> {
    qualify(Args::parse())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archived_consumer_comparison_covers_the_named_cases() -> Result<()> {
        let archive = marmish_inventory::root()
            .join("examples/marmish/evidence/stream-qualification/qualification.tar.gz");
        assert_eq!(
            file_sha(&archive)?,
            "7e1295d50933b2bf470c18055792e8ceba1748a12586cde8c21e636d7e205ec8"
        );
        let gzip = flate2::read::GzDecoder::new(fs::File::open(archive)?);
        let mut docs = Map::new();
        for entry in tar::Archive::new(gzip).entries()? {
            let entry = entry?;
            let name = entry.path()?.display().to_string();
            if matches!(
                name.as_str(),
                "original-consumers.json" | "candidate-consumers.json" | "qualification.json"
            ) {
                docs.insert(name, serde_json::from_reader(entry)?);
            }
        }
        let a = &docs["original-consumers.json"];
        let b = &docs["candidate-consumers.json"];
        consumers::compare(a, b)?;
        assert_eq!(
            docs["qualification.json"]["consumerScope"],
            consumers::scope()
        );
        for (name, cases) in object(&consumers::scope())? {
            assert_eq!(&b["suites"][name]["selected"], cases);
            for case in strings(cases)? {
                assert_eq!(b["suites"][name]["outcomes"][&case], "ok");
                let mut changed = b.clone();
                changed["suites"][name]["outcomes"][case] = json!("FAILED");
                assert!(consumers::compare(a, &changed).is_err());
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires retained locked native consumer builds on Apple Silicon"]
    fn rerun_retained_consumer_executables() -> Result<()> {
        let evidence =
            PathBuf::from(std::env::var("ODDLANG_STREAM_WORKSPACE_CAPTURE")?).canonicalize()?;
        let tools = Tools::discover()?;
        let helper = executable("provider-validation", "build-closure")?;
        for (role, workspace, writable, allow_fresh) in [
            ("candidate", "candidate", "a", true),
            ("original", "upstreams", "b", false),
        ] {
            let sandbox = evidence.join(format!("{role}-consumers/runtime.sb"));
            let previous = evidence.join(format!("{role}-consumers"));
            let work = evidence.join(writable);
            let scratch = tempfile::Builder::new()
                .prefix("verify-consumers-")
                .tempdir_in(&work)?;
            let env = tools.environment(&scratch.path().join("run"), &evidence.join(workspace))?;
            let suites = consumers::observe(
                &helper,
                &work.join("consumers"),
                &env,
                &sandbox,
                scratch.path(),
                &previous.join("consumer-build.stdout"),
                allow_fresh,
            )?;
            let stored = read_json(&evidence.join(format!("{role}-consumers.json")))?;
            assert_eq!(stored["suites"], suites, "{role} native linked suites");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires the pinned upstream baseline and previously generated stream workspace"]
    fn fresh_regenerated_package_and_negative_controls() -> Result<()> {
        let archived =
            PathBuf::from(std::env::var("ODDLANG_STREAM_WORKSPACE_CAPTURE")?).canonicalize()?;
        let regenerated =
            PathBuf::from(std::env::var("ODDLANG_STREAM_FRESH_CANDIDATE")?).canonicalize()?;
        let baseline = read_json(Path::new(&std::env::var("ODDLANG_STREAM_BASELINE")?))?;
        let temp = tempfile::tempdir()?;
        let out = temp.path().canonicalize()?;
        let src = out.join("candidate");
        // All mutations occur in this test's private copy of the emitted workspace.
        ensure!(
            Command::new("/bin/cp")
                .arg("-R")
                .arg(regenerated)
                .arg(&src)
                .status()?
                .success(),
            "copy fixture"
        );
        let a = out.join("a");
        let tools = Tools::discover()?;
        let env = tools.environment(&a, &src)?;
        let vendor = archived.join("preparation/vendor");
        fs::copy(
            archived.join("a/cargo/config.toml"),
            a.join("cargo/config.toml"),
        )?;
        let helper = executable("provider-validation", "build-closure")?;
        let build = out.join("build.sb");
        let bundle = src.join("oddlang/stream");
        candidate_profile(&helper, &tools, &src, &a, &bundle, &vendor, &build)?;
        let probe = denied_reads(
            &build,
            json!({"graph":bundle.join("syntax.ttl")}),
            &src,
            &env,
        )?;
        assert_eq!(probe["graph"]["returnCode"], 1);
        collect(&src, &a.join("stream"), &baseline, &env, &build, &vendor)?;
        let negative = controls(
            &src,
            &a,
            &env,
            &build,
            &vendor,
            &tools.chain.join("bin/cargo"),
            &baseline,
            &out,
        )?;
        assert_eq!(
            negative,
            read_json(&archived.join("qualification.json"))?["negativeControls"]
        );
        Ok(())
    }
}
