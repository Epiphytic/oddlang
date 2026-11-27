use super::*;
use provider_validation::pin;
use serde_json::json;

pub(super) fn verify(suite: &Suite) -> Result {
    suite.witness("records.rs", true, &[("GENERATED", "generated/lines.rs")])?;
    let root = suite.root();
    let scope = "urn:inventory:bad-log";
    let error = "urn:inventory:Fault";
    let alias = "urn:inventory:Result";
    let mut errors = json!({"format":"oddlang-finite-error-domain-v1","id":"urn:inventory:error-domain",
        "error":error,"result":alias,"categories":{},"variants":{(scope):{"payload":{"kind":"static-text"},
        "message":{"kind":"insertion","prefix":"invalid: ","suffix":""},"source":false,"from":false}}});
    write(&root.join("errors.json"), &errors)?;
    write(
        &root.join("errors-rust.json"),
        &json!({"format":"oddlang-finite-error-rust-v1","categoryTypes":{},
        "variantOrder":[scope],"names":{(error):"Fault",(alias):"Result",(scope):"Invalid"}}),
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"inventory-log\"\nversion = \"0.0.0\"\n",
    )?;
    let mut package = json!({"format":"oddlang-native-package-v1","name":"inventory-log","version":"0.0.0","edition":"2021",
        "dependencies":{},"cargoLock":pin(root,"Cargo.lock")?,"components":[
            {"kind":"finite-error-domain","module":"faults","graph":pin(root,"errors.json")?,"overlay":pin(root,"errors-rust.json")?,
                 "reexports":["Fault","Result"]},
            {"kind":"line-grammar","module":"logbook","graph":pin(root,"graph.json")?,"overlay":pin(root,"overlay.json")?,
                 "reexports":[],"crateReexports":["LogBook","Sample"]}]});
    write(&root.join("package.json"), &package)?;
    suite.build("native-package", &["package.json"], "product", None)?;
    run(
        Command::new("cargo").current_dir(root).args([
            "+1.97.1",
            "build",
            "--offline",
            "--locked",
            "--manifest-path",
            "product/Cargo.toml",
        ]),
        None,
    )?;
    let mut overlay = suite.fixture["overlay"].clone();
    for variant in ["Incorrect", "Invalid"] {
        overlay["error"]["variant"] = json!(variant);
        write(&root.join("overlay.json"), &overlay)?;
        package["components"][1]["overlay"] = pin(root, "overlay.json")?;
        if variant == "Invalid" {
            errors["variants"][scope]["payload"] = json!({"kind":"owned-text"});
            write(&root.join("errors.json"), &errors)?;
            package["components"][0]["graph"] = pin(root, "errors.json")?;
        }
        write(&root.join("package.json"), &package)?;
        suite.build(
            "native-package",
            &["package.json"],
            "rejected-package",
            Some("error binding does not match a native export"),
        )?;
    }
    Ok(())
}
