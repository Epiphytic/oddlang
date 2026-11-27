//! Qualify the shared-declaration model with the independent meter consumer.
use provider_validation::{Result, cargo, cli, command, invoke, pin, read, write};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../shared-declarations");
const ORACLE: &str = include_str!("../../../shared-declarations/oracle.rs");
fn project(
    cli: &Path,
    root: &Path,
    graph: &Value,
    binding: &Value,
    name: &str,
    error: Option<&str>,
) -> Result {
    write(&root.join("graph.json"), graph)?;
    write(&root.join("binding.json"), binding)?;
    invoke(
        cli,
        root,
        &[
            "build",
            "shared-declarations",
            "rust",
            "graph.json",
            "binding.json",
        ],
        name,
        error,
    )
}
fn same_projection(root: &Path, label: &str) -> Result {
    for name in ["shared.rs", "obligations.json", "projection.lock"] {
        assert_eq!(
            fs::read(root.join("good").join(name))?,
            fs::read(root.join(label).join(name))?,
            "{label}/{name}"
        );
    }
    Ok(())
}
fn check_projection(cli: &Path, root: &Path, g: &Value, b: &Value) -> Result {
    project(cli, root, g, b, "good", None)?;
    let mut reordered = g.clone();
    for pointer in [
        "/constants",
        "/errors/0/variants",
        "/stringEnums/0/variants",
    ] {
        reordered
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .ok_or(pointer.to_owned())?
            .reverse();
    }
    project(cli, root, &reordered, b, "replay", None)?;
    same_projection(root, "replay")?;
    let obligations = read(&root.join("good/obligations.json"))?;
    let rows = obligations["declarations"]
        .as_object()
        .ok_or("missing declarations")?;
    assert!(
        rows.keys()
            .eq(b["names"].as_object().ok_or("missing names")?.keys())
    );
    assert_eq!(rows["urn:meter:period"]["unsignedDecimal"], "51");
    assert_eq!(
        rows["urn:meter:label"]["examples"][0],
        json!({"payloadCanonicalText":"","display":"sensor {}"})
    );
    assert_eq!(rows["urn:meter:hot"]["wireString"], "h.ot");
    let mode = &rows["urn:meter:mode"];
    assert_eq!(mode["numberAdmission"]["display"], "sensor {mode code}");
    assert_eq!(
        mode["numberAdmission"]["reject"]
            .as_array()
            .ok_or("missing rejected tags")?
            .len(),
        254
    );
    assert!(
        !["COLD", "h.ot"].contains(
            &mode["rejectWireString"]
                .as_str()
                .ok_or("missing wire control")?
        )
    );
    assert_eq!(
        mode["nativeVariantOrder"],
        json!(["urn:meter:cold", "urn:meter:hot"])
    );
    let mut external = b.clone();
    external["externalErrorTypes"] = json!({"urn:meter:fault":"crate::provider::SensorError"});
    project(cli, root, g, &external, "external", None)?;
    let code = fs::read_to_string(root.join("external/shared.rs"))?;
    assert!(!code.contains("pub enum SensorError"));
    assert!(code.contains(r#"crate::provider::SensorError::Label("mode code")"#));
    let mut explicit = g.clone();
    explicit["stringEnums"][0]["copy"] = json!(true);
    project(cli, root, &explicit, b, "explicit-default", None)?;
    same_projection(root, "explicit-default")
}
fn reject(cli: &Path, root: &Path, g: &Value, b: &Value) -> Result {
    for (in_binding, pointer, value, error) in [
        (
            false,
            "/constants/0/value",
            json!({"ref":"urn:meter:period"}),
            "cyclic constant",
        ),
        (
            false,
            "/constants/1/value",
            json!({"multiply":[1u64<<63,2]}),
            "product overflow",
        ),
        (
            false,
            "/constants/1/value",
            json!(65536),
            "exceeds native type",
        ),
        (
            false,
            "/results/0/error",
            json!("urn:meter:absent"),
            "unresolved result error",
        ),
        (
            false,
            "/errors/0/variants/0/message",
            json!([{"payload":true}]),
            "typed display",
        ),
        (
            true,
            "/variantOrder/urn:meter:mode",
            json!(["urn:meter:cold", "urn:meter:hot", "urn:meter:cold"]),
            "variant order",
        ),
        (
            false,
            "/stringEnums/0/numbers/tags/urn:meter:hot",
            json!(3),
            "cover every variant uniquely",
        ),
        (
            false,
            "/stringEnums/0/numbers/unknown/id",
            json!("urn:meter:absent"),
            "unresolved number error",
        ),
        (
            false,
            "/stringEnums/0/numbers/unknown/id",
            json!("urn:meter:count"),
            "number error payload",
        ),
        (
            false,
            "/stringEnums/0/variants/1/wire",
            json!("h.ot"),
            "wire spelling",
        ),
        (
            true,
            "/names/urn:meter:mode",
            json!("Bad; fn"),
            "invalid Rust name",
        ),
    ] {
        let (mut graph, mut binding) = (g.clone(), b.clone());
        let doc = if in_binding { &mut binding } else { &mut graph };
        *doc.pointer_mut(pointer).ok_or(pointer.to_owned())? = value;
        project(cli, root, &graph, &binding, "denied", Some(error))?;
    }
    write(&root.join("graph.json"), g)?;
    for field in ["names", "constantTypes", "variantOrder"] {
        let (name, value) = b[field]
            .as_object()
            .and_then(|m| m.iter().next())
            .ok_or("missing map entry")?;
        let marker = format!(r#""{field}":{{"#);
        let dup = format!("{marker}{}:{value},", serde_json::to_string(name)?);
        fs::write(
            root.join("binding.json"),
            serde_json::to_string(b)?.replacen(&marker, &dup, 1),
        )?;
        invoke(
            cli,
            root,
            &[
                "build",
                "shared-declarations",
                "rust",
                "graph.json",
                "binding.json",
            ],
            "duplicate",
            Some("duplicate binding key"),
        )?;
    }
    Ok(())
}
fn package(cli: &Path, root: &Path, g: &Value, b: &Value, name: &str, oracle: &str) -> Result {
    write(&root.join("graph.json"), g)?;
    write(&root.join("binding.json"), b)?;
    let component = json!({"kind":"shared-declarations","module":"meter","graph":pin(root,"graph.json")?,"overlay":pin(root,"binding.json")?,
        "visibility":"public","reexports":["PERIOD","Reading","SensorError","Mode"],"rootReexports":["PERIOD","Reading","SensorError"],"crateReexports":["STEP"]});
    write(
        &root.join("package.json"),
        &json!({"format":"oddlang-native-package-v1","name":"thumbnail-consumer","version":"0.0.0","edition":"2021",
    "dependencies":{"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.154"},"base64":{"version":"0.22.1"}},"cargoLock":pin(root,"Cargo.lock")?,"components":[component]}),
    )?;
    invoke(
        cli,
        root,
        &["build", "native-package", "rust", "package.json"],
        name,
        None,
    )?;
    let out = root.join(name);
    assert!(fs::read_to_string(out.join("src/group_0/mod.rs"))?.contains("pub(crate) use"));
    fs::create_dir(out.join("tests"))?;
    fs::write(out.join("tests/oracle.rs"), oracle)?;
    let stdout = cargo(&out, &["test", "--locked", "--offline", "--test", "oracle"])?;
    assert!(String::from_utf8(stdout)?.contains("test inspect ... ok"));
    Ok(())
}
fn deny_caller(root: &Path, source: &str, diagnostic: &str) -> Result {
    fs::write(root.join("tests/bad.rs"), source)?;
    command(
        &mut Command::new("cargo"),
        root,
        &["check", "--locked", "--offline", "--test", "bad"],
        Some(diagnostic),
    )?;
    Ok(())
}
fn main() -> Result {
    let cli = cli()?;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let fixture = Path::new(FIXTURE);
    let g = read(&fixture.join("graph.json"))?;
    let b = read(&fixture.join("binding.json"))?;
    check_projection(&cli, root, &g, &b)?;
    reject(&cli, root, &g, &b)?;
    fs::copy(
        fixture.join("../../tests/fixtures/json-envelope/Cargo.lock"),
        root.join("Cargo.lock"),
    )?;
    package(&cli, root, &g, &b, "out", ORACLE)?;
    deny_caller(
        &root.join("out"),
        "use thumbnail_consumer::Mode;",
        "unresolved import",
    )?;
    deny_caller(
        &root.join("out"),
        "use thumbnail_consumer::meter::STEP;",
        "private",
    )?;
    let mut graph = g;
    graph["stringEnums"][0]["copy"] = json!(false);
    project(&cli, root, &graph, &b, "clone", None)?;
    assert_eq!(
        read(&root.join("clone/obligations.json"))?["declarations"]["urn:meter:mode"]["copy"],
        false
    );
    package(
        &cli,
        root,
        &graph,
        &b,
        "clone-package",
        &ORACLE.replace(
            "assert_eq!(Mode::Hot as u8,11);",
            "let mode = Mode::Hot; assert_eq!(mode.clone(), mode); assert_eq!(mode as u8,11);",
        ),
    )?;
    deny_caller(
        &root.join("clone-package"),
        "fn need<T: Copy>() {} #[test] fn copy() { need::<thumbnail_consumer::meter::Mode>(); }",
        "E0277",
    )?;
    println!(
        "Shared constants, errors, typed messages, aliases, native ordinals, serde wires and module visibility qualified."
    );
    Ok(())
}
