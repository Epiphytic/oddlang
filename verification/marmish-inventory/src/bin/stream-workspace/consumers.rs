use super::*;
use std::process::Stdio;
pub const PACKAGES: [&str; 3] = ["marmish-engine", "marmish-prototype", "marmish-native"];
pub fn scope() -> Value {
    json!({
        "marmish-engine/lib/marmish_engine":["copy_launch::tests::owned_context_is_read_only_cloexec_above_three_and_has_no_writer"],
        "marmish-prototype/test/bridge":["framing_is_bounded_and_diagnostics_are_sanitized"],
        "marmish-native/lib/marmish_native":["copy_io::tests::coalesced_prefix_and_data_are_bounded_and_delivered_in_order","copy_worker::tests::sender_data_record_fits_one_transport_block","stream_runner::tests::failed_exchange_does_not_apply_other_channel_ack","stream_session::tests::output_ack_regression_is_rejected_without_an_ipc_request"]
    })
}
pub fn run_capture(
    command: &[String],
    stem: &str,
    dir: &Path,
    env: &Environment,
    sandbox: &Path,
    logs: &Path,
) -> Result<i32> {
    Ok(Command::new("/usr/bin/sandbox-exec")
        .arg("-f")
        .arg(sandbox)
        .args(command)
        .current_dir(dir)
        .env_clear()
        .envs(env)
        .stdout(Stdio::from(fs::File::create(
            logs.join(format!("{stem}.stdout")),
        )?))
        .stderr(Stdio::from(fs::File::create(
            logs.join(format!("{stem}.stderr")),
        )?))
        .status()?
        .code()
        .unwrap_or(-1))
}
fn test_outcome(
    exe: &Path,
    name: &str,
    stem: &str,
    dir: &Path,
    env: &Environment,
    sandbox: &Path,
    logs: &Path,
) -> Result<&'static str> {
    let status = run_capture(
        &[
            exe.display().to_string(),
            "--test-threads=1".into(),
            "--exact".into(),
            name.into(),
        ],
        stem,
        dir,
        env,
        sandbox,
        logs,
    )?;
    let log = fs::read_to_string(logs.join(format!("{stem}.stdout")))?;
    let summary = regex::Regex::new(
        r"(?m)^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
    )?;
    let found = summary
        .captures_iter(&log)
        .last()
        .with_context(|| format!("test did not report a libtest outcome: {name} ({status})"))?;
    let counts = [
        found[1].parse::<usize>()?,
        found[2].parse()?,
        found[3].parse()?,
    ];
    ensure!(
        matches!(status, 0 | 101)
            && counts.iter().sum::<usize>() == 1
            && (status == 101) == (counts[1] == 1),
        "ambiguous test outcome: {name}"
    );
    Ok(["ok", "FAILED", "ignored"][counts.iter().position(|c| *c == 1).unwrap()])
}
fn listed(path: &Path) -> Result<Set> {
    Ok(stream_baseline::list_tests(&fs::read_to_string(path)?)
        .into_iter()
        .collect())
}

pub fn observe(
    helper: &Path,
    target: &Path,
    env: &Environment,
    sandbox: &Path,
    logs: &Path,
    artifacts: &Path,
    allow_fresh: bool,
) -> Result<Value> {
    let mut suites = Map::new();
    let scope = scope();
    for line in fs::read_to_string(artifacts)?.lines() {
        let row: Value = serde_json::from_str(line)?;
        if row["reason"] != "compiler-artifact"
            || row["executable"].is_null()
            || row["profile"]["test"] != true
        {
            continue;
        }
        ensure!(
            row["fresh"] == false || allow_fresh,
            "consumer executable unexpectedly reused"
        );
        let exe = Path::new(text(&row["executable"])?).canonicalize()?;
        ensure!(
            exe.starts_with(target),
            "unexpected executable path {}",
            exe.display()
        );
        let dir = Path::new(text(&row["manifest_path"])?)
            .parent()
            .context("package directory")?;
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .context("package name")?;
        ensure!(PACKAGES.contains(&name), "unexpected consumer {name}");
        let key = format!(
            "{name}/{}/{}",
            strings(&row["target"]["kind"])?.join(":"),
            text(&row["target"]["name"])?
        );
        ensure!(!suites.contains_key(&key), "ambiguous consumer suite {key}");
        let stem = format!("suite-{}", suites.len());
        let pin = file_sha(&exe)?;
        let mut e = env.clone();
        e.insert("CARGO_MANIFEST_DIR".into(), dir.display().to_string());
        e.insert("CARGO_PKG_NAME".into(), name.into());
        for (label, args) in [
            ("list", json!([exe, "--list"])),
            ("ignored", json!([exe, "--list", "--ignored"])),
        ] {
            checked(
                helper,
                args,
                &format!("{stem}-{label}"),
                dir,
                &e,
                logs,
                sandbox,
            )?;
        }
        ensure!(file_sha(&exe)? == pin, "consumer executable changed {key}");
        let list = listed(&logs.join(format!("{stem}-list.stdout")))?;
        let ignored = listed(&logs.join(format!("{stem}-ignored.stdout")))?;
        let cases = scope
            .get(&key)
            .map(strings)
            .transpose()?
            .unwrap_or_default();
        let mut outcomes = Map::new();
        for (i, case) in cases.iter().enumerate() {
            ensure!(
                list.contains(case) && !ignored.contains(case),
                "required test missing or ignored: {key}/{case}"
            );
            let observed =
                test_outcome(&exe, case, &format!("{stem}-{i}"), dir, &e, sandbox, logs)?;
            ensure!(observed == "ok", "required consumer test failed: {key}");
            outcomes.insert(case.clone(), json!(observed));
        }
        suites.insert(key,json!({"path":exe,"sha256":pin,"list":list,"ignored":ignored,"selected":cases,"outcomes":outcomes,"logs":stem}));
    }
    ensure!(
        PACKAGES
            .iter()
            .all(|pkg| suites.keys().any(|key| key.split('/').next() == Some(pkg)))
            && object(&scope)?.keys().all(|key| suites.contains_key(key)),
        "missing required consumer test targets or suites"
    );
    Ok(suites.into())
}
pub fn run(
    helper: &Path,
    root: &Path,
    writable: &Path,
    env: &Environment,
    sandbox: &Path,
    logs: &Path,
    cargo: &Path,
    check_workspace: bool,
) -> Result<Value> {
    fs::create_dir(logs)?;
    let target = writable.join("consumers");
    let mut env = env.clone();
    env.insert("CARGO_TARGET_DIR".into(), target.display().to_string());
    let mut commands = Map::new();
    if check_workspace {
        let cmd = json!([
            cargo,
            "check",
            "--locked",
            "--offline",
            "--workspace",
            "--all-targets"
        ]);
        checked(
            helper,
            cmd.clone(),
            "workspace-check",
            root,
            &env,
            logs,
            sandbox,
        )?;
        commands.insert("workspaceCheck".into(), cmd);
    }
    let mut command = vec![
        cargo.display().to_string(),
        "test".into(),
        "--locked".into(),
        "--offline".into(),
    ];
    for pkg in PACKAGES {
        command.extend(["-p".into(), pkg.into()]);
    }
    command.extend(["--message-format=json".into(), "--no-run".into()]);
    checked(
        helper,
        json!(command),
        "consumer-build",
        root,
        &env,
        logs,
        sandbox,
    )?;
    commands.insert("consumerBuild".into(), json!(command));
    let runtime = logs.join("runtime.sb");
    fs::write(
        &runtime,
        fs::read_to_string(sandbox)?
            + "(allow mach-lookup (global-name \"com.apple.system.opendirectoryd.libinfo\"))\n",
    )?;
    Ok(
        json!({"commands":commands,"suites":observe(helper,&target,&env,&runtime,logs,&logs.join("consumer-build.stdout"),check_workspace)?}),
    )
}
pub fn compare(a: &Value, b: &Value) -> Result<()> {
    ensure!(
        keys(&a["suites"])? == keys(&b["suites"])?,
        "consumer suites changed"
    );
    for (name, left) in object(&a["suites"])? {
        for field in ["list", "ignored", "selected", "outcomes"] {
            ensure!(
                left[field] == b["suites"][name][field],
                "drop-in test comparison differs: {name}.{field}"
            );
        }
    }
    Ok(())
}
