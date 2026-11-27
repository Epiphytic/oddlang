//! Exercise open and default-closed data records in a renamed native package.
use provider_validation::{Result, cargo, cli, invoke, pin, write};
use serde_json::{Value, json};
use std::{fs, io::Write};

fn main() -> Result {
    let cli = cli()?;
    let work = tempfile::tempdir()?;
    let root = work.path();
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='data-record-fixture'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nserde={version='=1.0.229',features=['derive']}\nserde_json='=1.0.151'\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    let graph: Value = serde_json::from_str(include_str!(
        "../../fixtures/public-records/read_policy/graph.json"
    ))?;
    let binding: Value = serde_json::from_str(include_str!(
        "../../fixtures/public-records/read_policy/binding.json"
    ))?;
    fs::write(
        root.join("choice.json"),
        include_bytes!("../../fixtures/public-records/read_policy/choice.json"),
    )?;
    fs::write(
        root.join("choice-rust.json"),
        include_bytes!("../../fixtures/public-records/read_policy/choice-binding.json"),
    )?;
    let project = |graph: &Value, overlay: &Value, label: &str, error: Option<&str>| -> Result {
        write(&root.join("record.json"), graph)?;
        write(&root.join("record-rust.json"), overlay)?;
        let package = json!({"format":"oddlang-native-package-v1","name":"data-record-fixture","version":"0.1.0","edition":"2021",
            "cargoLock":pin(root,"Cargo.lock")?,"dependencies":{"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.151"}},
            "components":[{"kind":"shared-declarations","module":"state_defs","reexports":["State"],"graph":pin(root,"choice.json")?,"overlay":pin(root,"choice-rust.json")?},
            {"kind":"public-records","module":"records","reexports":["Sample"],"graph":pin(root,"record.json")?,"overlay":pin(root,"record-rust.json")?}]});
        write(&root.join("package.json"), &package)?;
        invoke(
            &cli,
            root,
            &["build", "native-package", "rust", "package.json"],
            label,
            error,
        )
    };
    for (closed, label) in [(false, "open"), (true, "closed")] {
        let mut current = graph.clone();
        if closed {
            current["modules"][0]["records"][0]
                .as_object_mut()
                .unwrap()
                .remove("readPolicy");
        }
        project(&current, &binding, label, None)?;
        let product = root.join(label);
        let mut lib = fs::OpenOptions::new()
            .append(true)
            .open(product.join("src/lib.rs"))?;
        lib.write_all(
            include_str!("../../fixtures/public-records/read_policy/case.rs")
                .replace("CLOSED", &closed.to_string())
                .as_bytes(),
        )?;
        let output = cargo(
            &product,
            &[
                "test",
                "--offline",
                "--locked",
                "--lib",
                "--",
                "--exact",
                "wire_policy_examples",
            ],
        )?;
        assert!(String::from_utf8(output)?.contains("1 passed"));
    }
    let mut wrong = binding.clone();
    wrong["externalTypes"]["urn:fixture:state"] = json!(["crate", "Other"]);
    project(
        &graph,
        &wrong,
        "denied",
        Some("external public record choice"),
    )?;
    for (label, key, value, error) in [
        (
            "policy-without-deserializer",
            "traits",
            json!(["serialize"]),
            "read policy requires Deserialize",
        ),
        (
            "unsafe-input-shape",
            "readPolicy",
            json!({"shape":"unordered-sequence","unknownFields":"ignore"}),
            "unknown variant",
        ),
        (
            "equality-without-partial",
            "traits",
            json!(["eq", "deserialize"]),
            "Eq requires PartialEq",
        ),
    ] {
        let mut current = graph.clone();
        current["modules"][0]["records"][0][key] = value;
        project(&current, &binding, label, Some(error))?;
    }
    println!("Verified open and closed native readers and four rejection controls");
    Ok(())
}
