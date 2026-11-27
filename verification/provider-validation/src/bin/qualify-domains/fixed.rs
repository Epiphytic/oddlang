use super::*;
const VALUE: &str = "urn:sample:calibration";
const FAILURE: &str = "urn:sample:width-error";
const FAULT: &str = "urn:example:detector:invalid";
fn check_extent(cli: &Path, root: &Path, width: usize, host: &str) -> Result {
    let dir = root.join(width.to_string());
    fs::create_dir_all(dir.join("src"))?;
    fs::copy(
        fixtures().join("fixed-Cargo.toml.in"),
        dir.join("Cargo.toml"),
    )?;
    for (src, dest) in [
        ("fixed-reference.rs.in", "src/lib.rs"),
        ("fixed-caller.rs.in", "src/main.rs"),
    ] {
        fs::write(
            dir.join(dest),
            fs::read_to_string(fixtures().join(src))?.replace("WIDTH", &width.to_string()),
        )?;
    }
    cargo(&dir, &["generate-lockfile", "--offline"])?;
    let manifest = dir.join("Cargo.toml");
    let bytes = command(
        provider_validation::capture_command().env("ODD_RECOVERY_DATA_SHAPES", "1"),
        &dir,
        &[
            manifest.to_str().unwrap(),
            "instrument",
            "lib",
            "instrument",
            host,
            "default",
        ],
        None,
    )?;
    fs::write(dir.join("observed.json"), &bytes)?;
    let selection = fixtures().join("fixed-selection.json");
    invoke(
        cli,
        &dir,
        &[
            "recover",
            "fixed-bytes",
            "observed.json",
            selection.to_str().unwrap(),
        ],
        "recovered",
        None,
    )?;
    let graph = read(&dir.join("recovered/graph.json"))?;
    assert_eq!(
        graph["types"][VALUE],
        json!({"kind":"fixed-bytes","length":width,"onDrop":"erase","failure":FAILURE})
    );
    let lock = read(&dir.join("recovered/recovery.lock"))?;
    assert_eq!(lock["observed"]["length"], width);
    assert!(
        lock["authored"].get("failure").is_some()
            || lock["authored"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("failure")))
    );
    let mut wrong: Value = serde_json::from_slice(&bytes)?;
    wrong["crateName"] = json!("different");
    write(&dir.join("wrong.json"), &wrong)?;
    invoke(
        cli,
        &dir,
        &[
            "recover",
            "fixed-bytes",
            "wrong.json",
            selection.to_str().unwrap(),
        ],
        "invalid",
        Some("compiler or crate"),
    )?;
    build(
        cli,
        &dir,
        "checked-values",
        "recovered/graph.json",
        fixtures().join("fixed-rust.json").to_str().unwrap(),
        "product",
        None,
    )?;
    let args = ["run", "--quiet", "--offline", "--locked"];
    let original = cargo(&dir, &args)?;
    fs::write(
        dir.join("src/lib.rs"),
        "pub enum Fault { Width }\nmod projected; pub use projected::Calibration;\n",
    )?;
    fs::copy(dir.join("product/values.rs"), dir.join("src/projected.rs"))?;
    let actual = cargo(&dir, &args)?;
    assert_eq!(actual, original);
    let text = String::from_utf8(actual)?;
    let rows: Vec<_> = text.lines().collect();
    assert_eq!(rows.len(), 13);
    assert!(rows[width].ends_with(&format!("{:?}", vec![3; width])));
    for bound in ["Clone", "Copy", "std::fmt::Debug", "PartialEq"] {
        fs::write(
            dir.join("src/main.rs"),
            format!("fn must<T: {bound}>() {{}} fn main() {{must::<instrument::Calibration>();}}"),
        )?;
        let output = Command::new("cargo")
            .args([
                "+1.97.1",
                "check",
                "--offline",
                "--locked",
                "--message-format=json",
            ])
            .current_dir(&dir)
            .output()?;
        assert!(!output.status.success());
        let rows: Vec<Value> = std::str::from_utf8(&output.stdout)?
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        assert!(
            rows.iter()
                .any(|r| r["reason"] == "compiler-message"
                    && r["message"]["code"]["code"] == "E0277")
        );
    }
    Ok(())
}
pub fn verify(cli: &Path, root: &Path) -> Result {
    let version = String::from_utf8(command(&mut Command::new("rustc"), root, &["-vV"], None)?)?;
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("missing rustc host")?;
    for width in [7, 9] {
        check_extent(cli, root, width, host)?;
    }
    let shared = component(
        root,
        "shared-declarations",
        "faults",
        &["Fault"],
        fixture("errors.json")?,
        fixture("error-rust.json")?,
    )?;
    let graph = json!({"format":"oddlang-checked-values-v1","id":"urn:sample:calibration-domain","types":{VALUE:{"kind":"fixed-bytes","length":7,"onDrop":"erase","failure":FAULT}}});
    let mut binding = fixture("fixed-rust.json")?;
    binding["errors"] = json!({FAULT:{"errorType":"crate::Fault","variant":"Invalid"}});
    let fixed = component(
        root,
        "checked-values",
        "fixed",
        &["Calibration"],
        graph,
        binding,
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"instrument\"\nversion=\"1.0.0\"\n",
    )?;
    let git = json!({"git":"https://example.org/provider","revision":"1".repeat(40)});
    let expected = "locked registry provider";
    for (dep, patch, error) in [
        (json!({}), json!({}), expected),
        (json!({"version":"1.8.0"}), json!({}), expected),
        (json!({"version":"1.9.1","source":git}), json!({}), expected),
        (json!({"version":"1.9.1"}), json!({"zeroize":git}), expected),
        (
            json!({"version":"1.9.1"}),
            json!({}),
            "expected locked registry source and checksum",
        ),
    ] {
        let mut spec = package("instrument", "1.0.0", "2021", root, json!([shared, fixed]))?;
        spec["dependencies"] = if dep == json!({}) {
            json!({})
        } else {
            json!({"zeroize":dep})
        };
        spec["registryPatches"] = patch;
        assemble(cli, root, &spec, "rejected", Some(error))?;
    }
    println!(
        "Recovered 7 and 9 octets; both native bounds, copy ownership, borrowing, drop provider and trait controls passed."
    );
    Ok(())
}
