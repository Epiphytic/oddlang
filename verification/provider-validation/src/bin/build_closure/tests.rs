use super::*;
fn fixture(root: &Path) -> Result<(PathBuf, Value)> {
    let src = root.join("src");
    let mut rows = Vec::new();
    for rel in [FRAME, SCRIPT, "Cargo.lock"] {
        let file = src.join(rel);
        fs::create_dir_all(file.parent().unwrap())?;
        fs::write(&file, "fixture")?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644))?;
        rows.push(json!({"path":rel,"sha256":digest(b"fixture"),"mode":"100644","producer":if rel==FRAME{"generated"}else{"retained"}}));
    }
    Ok((
        src,
        json!({"cargoLockSha256":digest(b"fixture"),"files":rows}),
    ))
}
#[test]
fn undeclared_file_and_symlink_fail_before_build() -> Result {
    let stage_dir = tempfile::tempdir()?;
    let root = stage_dir.path();
    let (src, manifest) = fixture(root)?;
    let extra = src.join("undeclared");
    for link in [false, true] {
        if link {
            symlink(src.join(FRAME), &extra)?;
        } else {
            fs::write(&extra, "unlisted")?;
        }
        assert!(
            stage(&src, &manifest, &root.join("copy"))
                .unwrap_err()
                .to_string()
                .contains("undeclared or special")
        );
        fs::remove_file(&extra)?;
    }
    Ok(())
}
#[test]
fn compiler_must_observe_generated_and_retained_sources() -> Result {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let (src, _) = fixture(&root)?;
    let target = root.join("target");
    fs::create_dir(&target)?;
    let d = target.join("unit.d");
    let entries = [(FRAME.into(), Value::Null), (SCRIPT.into(), Value::Null)]
        .into_iter()
        .collect();
    let inspect = || {
        depinfo(
            &src,
            &target,
            &root.join("vendor"),
            &root.join("rust"),
            &root.join("sdk"),
            &entries,
        )
    };
    fs::write(&d, format!("unit: {FRAME}\n"))?;
    assert!(
        inspect()
            .unwrap_err()
            .to_string()
            .contains("native build.rs")
    );
    fs::write(&d, format!("unit: {FRAME} {SCRIPT}\n"))?;
    assert_eq!(inspect()?.1, entries.keys().cloned().collect());
    fs::write(&d, format!("unit: {FRAME} {}/unlisted\n", root.display()))?;
    assert!(
        inspect()
            .unwrap_err()
            .to_string()
            .contains("escaped declared roots")
    );
    Ok(())
}
#[test]
fn rustc_make_escapes() -> Result {
    assert_eq!(
        dep_paths("target: some\\ file.rs file.rs\n\n# env-dep:X=abc\n")?,
        ["some file.rs", "file.rs"]
    );
    Ok(())
}
