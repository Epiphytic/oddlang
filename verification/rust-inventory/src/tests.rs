use super::*;

#[test]
fn registry_cache_matches_versioned_vendor() -> Result<()> {
    let cache = PathBuf::from("/cargo/registry/src");
    let vendor = PathBuf::from("/workspace-vendor");
    let rel = "some-library-0.1.0/src/lib.rs";
    let roots = vec![
        ("registry-cache", cache.clone()),
        ("registry", vendor.clone()),
    ];
    let producers = trace::Producers::new();
    assert_eq!(
        trace::identity(&cache.join("index-root").join(rel), &roots, &producers)?,
        trace::identity(&vendor.join(rel), &roots, &producers)?
    );
    Ok(())
}

#[test]
fn package_id_survives_workspace_relocation() {
    for root in ["/original", "/emitted"] {
        assert_eq!(
            trace::package_id(&format!("path+file://{root}#probe@0.1.0"), Path::new(root)),
            "workspace:#probe@0.1.0"
        );
        assert_eq!(
            trace::package_id(
                &format!("path+file://{root}/crates/probe#0.1.0"),
                Path::new(root)
            ),
            "workspace:crates/probe#0.1.0"
        );
    }
}

#[test]
fn changed_source_invalidates_bundle() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let base = temp.path().canonicalize()?;
    let source = base.join("lib.rs");
    fs::write(&source, "const PRESENT: u32 = 1;")?;
    let graph = base.join("syntax.ttl");
    fs::write(&graph, "<urn:test> <urn:value> \"PRESENT\" .\n")?;
    let inventory = base.join("sources.json");
    fs::write(
        &inventory,
        json!({"files":{"lib.rs":sha(&source)?}}).to_string(),
    )?;
    let lock = base.join("bundle.lock");
    fs::write(
        &lock,
        json!({"version":1,"syntaxSha256":sha(&graph)?,"sourcesSha256":sha(&inventory)?})
            .to_string(),
    )?;
    let pin = sha(&lock)?;
    assert_eq!(evidence::bundle(&base, &pin, &base)?["sourcesMatched"], 1);
    fs::write(&source, "const PRESENT: u32 = 2;")?;
    assert_eq!(
        evidence::bundle(&base, &pin, &base)
            .unwrap_err()
            .to_string(),
        "stale syntax bundle source digest: lib.rs"
    );
    Ok(())
}

#[test]
fn multiline_cargo_commands_keep_quoted_values() -> Result<()> {
    let log = "  Running `A='line\nnext' tool --cfg '#one two' \"a\\$b\" ''`\n";
    assert_eq!(
        trace::commands(log)?,
        vec![vec![
            "A=line\nnext",
            "tool",
            "--cfg",
            "#one two",
            "a\\$b",
            ""
        ]]
    );
    assert!(trace::commands(" Running `A='incomplete").is_err());
    Ok(())
}
