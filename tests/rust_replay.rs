use std::{fs, path::Path, process::Command};

#[test]
fn replay_checks_the_actual_inputs_and_emitted_files() {
    let root = Path::new("examples/rust-import/ledger");
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path().join("project");
    let output = scratch.path().join("native");
    let manifest = root.join("Cargo.toml");
    let mapping = root.join("mapping.toml");
    let run = |args: &[&Path]| {
        Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(args)
            .output()
            .unwrap()
    };
    for args in [
        vec![
            Path::new("import"),
            Path::new("rust"),
            &manifest,
            &mapping,
            &dir,
        ],
        vec![
            Path::new("import"),
            Path::new("replay"),
            &manifest,
            &mapping,
            &dir,
        ],
        vec![Path::new("build"), &dir, &output],
        vec![
            Path::new("import"),
            Path::new("verify-build"),
            &dir,
            &output,
        ],
    ] {
        let result = run(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fs::write(output.join("account.rs"), "// wrong output").unwrap();
    assert!(
        !run(&[
            Path::new("import"),
            Path::new("verify-build"),
            &dir,
            &output
        ])
        .status
        .success()
    );
}
