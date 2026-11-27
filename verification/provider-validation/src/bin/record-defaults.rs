//! Compare recovered field defaults, native ordering, and macro visibility.
use provider_validation::{Result, cli, command, invoke, read, sha, write};
use serde_json::json;
use std::{fs, path::Path, process::Command};

const SOURCE: &str = include_str!("../../fixtures/record-defaults/source.rs");
const PROBE: &str = include_str!("../../fixtures/record-defaults/probe.rs");

fn recover(cli: &Path, root: &Path, source: &str, rejection: Option<&str>) -> Result {
    fs::create_dir(root)?;
    let input = root.join("input.rs");
    fs::write(&input, source)?;
    write(
        &root.join("selection.json"),
        &json!({
            "format":"oddlang-rust-selected-record-selection-v1","id":"urn:example:cartography",
            "sourceSha256":sha(&input)?,"records":{"CartographicSample":"urn:example:cartography:sample"}
        }),
    )?;
    invoke(
        cli,
        root,
        &[
            "recover",
            "public-root-records",
            "input.rs",
            "selection.json",
        ],
        "recovered",
        rejection,
    )
}

fn project(cli: &Path, root: &Path) -> Result {
    invoke(
        cli,
        root,
        &[
            "build",
            "public-records",
            "rust",
            "recovered/graph.json",
            "recovered/rust.json",
        ],
        "projection",
        None,
    )
}

fn compile(root: &Path, source: &str, rejection: Option<&str>) -> Result {
    fs::write(root.join("probe.rs"), source)?;
    command(
        &mut Command::new("rustc"),
        root,
        &["--edition=2021", "probe.rs"],
        rejection,
    )?;
    Ok(())
}

fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    for (name, source) in [
        ("baseline", SOURCE.to_string()),
        (
            "changed",
            SOURCE.replace(
                "    pub waypoints: u32,\n    pub points: u64,",
                "    pub points: u64,\n    pub waypoints: u32,",
            ),
        ),
    ] {
        let work = root.join(name);
        recover(&cli, &work, &source, None)?;
        project(&cli, &work)?;
        compile(&work, PROBE, None)?;
        let observed = String::from_utf8(command(
            &mut Command::new(work.join("probe")),
            &work,
            &[],
            None,
        )?)?;
        let first = observed.find("waypoints:").ok_or("missing waypoints")?;
        let second = observed.find(" points: ").ok_or("missing points")?;
        assert_eq!(first < second, name == "baseline");
        assert!(
            observed.contains("current: false") && observed.contains(r#"reference: "../unknown""#)
        );
    }
    for (name, source, error) in [
        (
            "shadow",
            format!("{SOURCE}\npub enum u32 {{ Example }}"),
            "shadowed public record",
        ),
        (
            "default-shadow",
            format!("{SOURCE}\nuse core::clone::Clone as Default;"),
            "shadowed public record",
        ),
        (
            "missing-path",
            SOURCE.replace("use std::path::PathBuf;", ""),
            "unresolved public path field",
        ),
        (
            "extra-impl",
            format!("{SOURCE}\nimpl CartographicSample {{ pub fn extra() {{}} }}"),
            "unsupported macros, aliases or impls",
        ),
        (
            "serialized-path",
            format!(
                "use serde::Serialize;\n{}",
                SOURCE.replace("Debug, Default", "Debug, Default, Serialize")
            ),
            "serialized public path field has no portable wire rule",
        ),
    ] {
        recover(&cli, &root.join(name), &source, Some(error))?;
    }
    let work = root.join("baseline");
    let mut graph = read(&work.join("recovered/graph.json"))?;
    graph["modules"][0]["records"][0]["traits"]
        .as_array_mut()
        .ok_or("missing traits")?
        .push(json!("serialize"));
    write(&work.join("serialized.json"), &graph)?;
    invoke(
        &cli,
        &work,
        &[
            "build",
            "public-records",
            "rust",
            "serialized.json",
            "recovered/rust.json",
        ],
        "invalid-wire",
        Some("serialized public path field has no portable wire rule"),
    )?;
    let shadowed = SOURCE
        .replace(
            "struct $name;",
            "#[derive(Clone, Debug, Default)] struct $name;",
        )
        .replace("unit!(Hidden);", "unit!(u32);");
    let work = root.join("macro-shadow");
    recover(&cli, &work, &shadowed, None)?;
    let lock = read(&work.join("recovered/recovery.lock"))?;
    assert!(
        lock["gap"]
            .as_str()
            .ok_or("missing gap")?
            .contains("macro expansion")
    );
    compile(
        &work,
        &format!(
            "{shadowed}\nfn main() {{ let _: ::core::primitive::u32 = CartographicSample::default().waypoints; }}"
        ),
        Some("mismatched types"),
    )?;
    let extra = format!(
        "{SOURCE}{}",
        include_str!("../../fixtures/record-defaults/extra.rs")
    );
    let work = root.join("macro-impl");
    recover(&cli, &work, &extra, None)?;
    let comparison =
        "fn main() { assert_eq!(CartographicSample::default(), CartographicSample::default()); }";
    compile(&work, &format!("{extra}\n{comparison}"), None)?;
    project(&cli, &work)?;
    compile(
        &work,
        &format!("include!(\"projection/records.rs\");\n{comparison}"),
        Some("binary operation"),
    )
}
