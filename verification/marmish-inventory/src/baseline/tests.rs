use super::*;
#[test]
fn archive_transcripts_bind_all_command_ids_and_binaries() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("tar")
            .arg("-xzf")
            .arg(root().join("examples/marmish/evidence/qualification.tar.gz"))
            .arg("-C")
            .arg(dir.path())
            .arg("native")
            .status()
            .unwrap()
            .success()
    );
    let historic = read_json(&dir.path().join("native/original/native/baseline.json")).unwrap();
    let published = read_json(&dir.path().join("native/qualification.json")).unwrap();
    let result = candidate::compare(
        &dir.path().join("native"),
        PIN,
        text(&published["candidateCommit"]).unwrap(),
        text(&published["candidateTree"]).unwrap(),
        candidate::LEGACY_VERIFIER,
    )
    .unwrap();
    assert_eq!(result, published);
    assert_eq!(
        keys(&historic["binaries"]).unwrap(),
        set(&["marmish", "marmishd", "marmish-worker"])
    );
    assert_eq!(historic["timeoutSeconds"], 900);
    assert_eq!(
        historic["isolation"]["profileSha256"],
        file_digest(&root().join("scripts/marmish/baseline-macos.sb")).unwrap()
    );
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod macos {
    use super::*;
    use std::io::Write;
    #[test]
    #[ignore = "requires native Apple Silicon, pinned Rust, Python 3.12, uv and Xcode"]
    fn fixture_runs_complete_native_matrix_and_terminates_timed_out_group() {
        let tmp = tempfile::Builder::new()
            .prefix("m-")
            .tempdir_in("/private/tmp")
            .unwrap();
        let repo = tempfile::tempdir().unwrap();
        let src = repo.path();
        fs::create_dir_all(src.join("src/bin")).unwrap();
        fs::create_dir_all(src.join("tools/code-ontology")).unwrap();
        fs::write(src.join("Cargo.toml"),"[package]\nname='marmish-native'\nversion='1.0.0'\nedition='2024'\n[features]\ninstalled-checks=[]\n").unwrap();
        fs::write(
            src.join("rust-toolchain.toml"),
            "[toolchain]\nchannel='1.97.1'\n",
        )
        .unwrap();
        fs::write(
            src.join("src/lib.rs"),
            "#[test] fn observed() {}\n#[test] #[ignore = \"fixture\"] fn deferred() {}\n",
        )
        .unwrap();
        fs::write(src.join("build.rs"),r#"fn main(){
            let rev=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();
            println!("cargo:rustc-env=MARMISH_BUILD_COMMIT={}",std::str::from_utf8(&rev.stdout).unwrap().trim());
            println!("cargo:rustc-env=MARMISH_BUILD_TARGET={}",std::env::var("TARGET").unwrap());
        }"#).unwrap();
        for name in [
            "marmish",
            "marmishd",
            "marmish-worker",
            "marmish-release-tool",
        ] {
            fs::write(
                src.join(format!("src/bin/{name}.rs")),
                include_str!("fixture-bin.txt"),
            )
            .unwrap();
        }
        fs::write(src.join("tools/code-ontology/pyproject.toml"),"[project]\nname='matrix-fixture'\nversion='1.0.0'\nrequires-python='>=3.12,<3.13'\ndependencies=[]\n").unwrap();
        // The baseline invokes the upstream project's Python index. Its fixture
        // records the exact arguments supplied by the native collector.
        fs::write(
            src.join("tools/code-ontology/index.py"),
            "import sys\nassert sys.argv[1:] == ['check', '--ref', 'HEAD']\n",
        )
        .unwrap();
        assert!(
            Command::new("uv")
                .args([
                    "lock",
                    "--offline",
                    "--python",
                    "3.12",
                    "--project",
                    "tools/code-ontology"
                ])
                .current_dir(src)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("cargo")
                .args(["+1.97.1", "generate-lockfile", "--offline"])
                .current_dir(src)
                .status()
                .unwrap()
                .success()
        );
        for cmd in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.email=fixture@invalid",
                "-c",
                "user.name=Fixture",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-qm",
                "fixture",
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(cmd)
                    .current_dir(src)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let revision = tool("git", &["-C", src.to_str().unwrap(), "rev-parse", "HEAD"]).unwrap();
        let report = tempfile::tempdir().unwrap();
        let mut runner = Runner::setup(src, tmp.path(), report.path(), &revision).unwrap();
        runner.matrix(true).unwrap();
        let state = read_json(&report.path().join("baseline.json")).unwrap();
        let rows = array(&state["commands"]).unwrap();
        assert_eq!(rows.len(), 22);
        assert!(
            rows.iter().all(|r| r["status"] == "passed"),
            "{:?}",
            rows.iter()
                .filter(|r| r["exitCode"] != 0)
                .map(|r| (
                    &r["name"],
                    fs::read_to_string(report.path().join(text(&r["stderr"]).unwrap()))
                ))
                .collect::<Vec<_>>()
        );
        let published = Command::new("tar")
            .arg("-xOf")
            .arg(root().join("examples/marmish/evidence/original.tar.gz"))
            .arg("native/baseline.json")
            .output()
            .unwrap();
        let historical: Value = serde_json::from_slice(&published.stdout).unwrap();
        for (actual, reference) in rows.iter().zip(array(&historical["commands"]).unwrap()) {
            for key in ["name", "argv", "stdout", "stderr"] {
                assert_eq!(actual[key], reference[key], "{} {key}", actual["name"]);
            }
            for stream in ["stdout", "stderr"] {
                assert_eq!(
                    actual[format!("{stream}Sha256")],
                    file_digest(&report.path().join(text(&actual[stream]).unwrap())).unwrap()
                );
            }
        }
        assert_eq!(state["tools"], historical["tools"]);
        assert_eq!(state["flags"], historical["flags"]);
        assert_eq!(state["isolation"], historical["isolation"]);
        assert_eq!(state["platforms"], historical["platforms"]);
        assert_eq!(state["installedStatus"]["stdoutMatchesExpected"], true);
        assert_eq!(state["installedStatus"]["fixtureUnchanged"], true);
        assert_eq!(state["installedStatus"]["exitCode"], 0);
        assert_eq!(
            keys(&state["binaries"]).unwrap(),
            set(&["marmish", "marmishd", "marmish-worker"])
        );
        let pidfile = tmp.path().join("pid");
        let script = format!("sleep 50 & echo $! > {}; wait", pidfile.display());
        assert_eq!(
            runner
                .observe(
                    "timeout",
                    &args(&["/bin/sh", "-c", &script]),
                    Duration::from_millis(350)
                )
                .unwrap(),
            -libc::SIGKILL
        );
        let timeout = runner.state["commands"].as_array().unwrap().last().unwrap();
        assert_eq!(timeout["timedOut"], true);
        assert_eq!(timeout["status"], "timed-out");
        let pid: i32 = fs::read_to_string(pidfile).unwrap().trim().parse().unwrap();
        let mut killed = false;
        for _ in 0..40 {
            if unsafe { libc::kill(pid, 0) } == -1
                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                killed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(killed, "timed-out descendant remained alive");
        let mut forbidden = tempfile::NamedTempFile::new().unwrap();
        forbidden.write_all(b"sentinel").unwrap();
        let cmd = format!("echo changed > {}", forbidden.path().display());
        assert_ne!(
            runner
                .command("denied-write", &["/bin/sh", "-c", &cmd])
                .unwrap(),
            0
        );
        assert_eq!(fs::read(forbidden.path()).unwrap(), b"sentinel");
    }
    #[test]
    #[ignore = "requires a cached checkout at MARMISH_PINNED_SOURCE"]
    fn actual_pinned_source_ontology_metadata_and_contracts() {
        let src = PathBuf::from(std::env::var("MARMISH_PINNED_SOURCE").unwrap());
        let tmp = tempfile::Builder::new()
            .prefix("m-")
            .tempdir_in("/private/tmp")
            .unwrap();
        let report = tempfile::tempdir().unwrap();
        let mut runner = Runner::setup(&src, tmp.path(), report.path(), PIN).unwrap();
        runner.probes().unwrap();
        let state = read_json(&report.path().join("baseline.json")).unwrap();
        let rows = array(&state["commands"]).unwrap();
        assert_eq!(rows.len(), 11);
        for row in rows {
            assert_eq!(
                row["status"],
                "passed",
                "{}: {}",
                row["name"],
                fs::read_to_string(report.path().join(text(&row["stderr"]).unwrap())).unwrap()
            );
        }
        let pinned = Command::new("tar")
            .arg("-xOf")
            .arg(root().join("examples/marmish/evidence/original.tar.gz"))
            .arg("native/baseline.json")
            .output()
            .unwrap();
        let original: Value = serde_json::from_slice(&pinned.stdout).unwrap();
        for key in [
            "revision",
            "osVersion",
            "tree",
            "tools",
            "locks",
            "flags",
            "isolation",
            "platforms",
        ] {
            assert_eq!(state[key], original[key], "{key}");
        }
        assert_eq!(
            rows[8]["stdoutSha256"],
            original["commands"][8]["stdoutSha256"]
        );
        // These seven original contract tests exercise pure framing, URL validation,
        // and configuration bounds inside the same isolated collector environment.
        assert_eq!(
            runner
                .cargo(
                    "core-contracts",
                    "test",
                    &[
                        "-p",
                        "marmish-core",
                        "--test",
                        "contracts",
                        "--no-fail-fast"
                    ]
                )
                .unwrap(),
            0
        );
        let native = fs::read_to_string(report.path().join("logs/core-contracts.stdout")).unwrap();
        assert!(
            native
                .lines()
                .any(|line| line.starts_with("test result: ok. 7 passed; 0 failed; 0 ignored;"))
        );
    }
}
