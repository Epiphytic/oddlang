//! Verify the standalone reader, recovered link bound and package-level native error roles.
use provider_validation::{Result, cargo, cli, invoke, pin, read, sha, write};
use serde_json::json;
use std::{fs, path::Path};
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/read-only-file");
    for file in [
        "graph.json",
        "rust.json",
        "observation.json",
        "error.json",
        "error-rust.json",
    ] {
        fs::copy(fixtures.join(file), root.join(file))?;
    }
    let project = ["build", "read-only-file", "rust", "graph.json", "rust.json"];
    invoke(&cli, root, &project, "projection", None)?;
    fs::create_dir(root.join("src"))?;
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../read-only-file/caller.rs"),
        root.join("src/lib.rs"),
    )?;
    fs::copy(root.join("projection/file.rs"), root.join("src/file.rs"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='inert-sample'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nlibc='=0.2.189'\nsha2='=0.10.9'\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    cargo(root, &["test", "--locked", "--offline"])?;
    let graph = read(&root.join("graph.json"))?;
    write(
        &root.join("selection.json"),
        &json!({"format":"oddlang-read-only-file-selection-v1",
        "observationSha256":sha(&root.join("observation.json"))?,"profile":graph["profile"]}),
    )?;
    invoke(
        &cli,
        root,
        &[
            "recover",
            "read-only-file",
            "observation.json",
            "selection.json",
        ],
        "recovered",
        None,
    )?;
    let mut expected = graph.clone();
    expected["maximumLinks"] = 2.into();
    assert_eq!(read(&root.join("recovered/graph.json"))?, expected);
    let mut reversed = graph["profile"]["guards"].clone();
    reversed.as_array_mut().unwrap().reverse();
    for (pointer, value) in [
        ("/maximumLinks", json!(0)),
        ("/profile/modeMask", json!(4095)),
        ("/profile/safety/nonblockingOpen", json!(false)),
        ("/profile/open", json!(["read-only", "close-on-exec"])),
        ("/profile/guards", reversed),
        ("/profile/digest/chunkBytes", json!(0)),
    ] {
        let mut bad = graph.clone();
        *bad.pointer_mut(pointer).ok_or("missing fixture property")? = value;
        write(&root.join("graph.json"), &bad)?;
        invoke(&cli, root, &project, "negative", Some(""))?;
    }
    write(&root.join("graph.json"), &graph)?;
    let mut manifest = json!({"format":"oddlang-native-package-v1","name":"inert-sample","version":"0.1.0","edition":"2021","cargoLock":pin(root,"Cargo.lock")?,
        "dependencies":{"libc":{"version":"0.2.189"},"sha2":{"version":"0.10.9"}},
        "components":[{"kind":"finite-error-domain","module":"fault","reexports":["Fault","Result"],"graph":pin(root,"error.json")?,"overlay":pin(root,"error-rust.json")?},
            {"kind":"read-only-file","module":"descriptor","reexports":[],"crateReexports":["open_sample","read_sample","check_sample"],
             "graph":pin(root,"graph.json")?,"overlay":pin(root,"rust.json")?}]});
    let package = ["build", "native-package", "rust", "package.json"];
    write(&root.join("package.json"), &manifest)?;
    invoke(&cli, root, &package, "native", None)?;
    cargo(&root.join("native"), &["check", "--offline", "--locked"])?;
    let binding = read(&root.join("rust.json"))?;
    for (label, key, value, diagnostic) in [
        (
            "io-identity",
            "ioVariant",
            json!("Rejected"),
            "invalid read-only file Rust binding",
        ),
        (
            "unresolved-variant",
            "ioVariant",
            json!("Missing"),
            "error role does not match its bound native variant",
        ),
        (
            "guessed-type",
            "error",
            json!(["crate", "Missing"]),
            "error role does not match its bound native variant",
        ),
    ] {
        let mut bad = binding.clone();
        bad[key] = value;
        write(&root.join("rust.json"), &bad)?;
        manifest["components"][1]["overlay"] = pin(root, "rust.json")?;
        write(&root.join("package.json"), &manifest)?;
        invoke(&cli, root, &package, label, Some(diagnostic))?;
    }
    Ok(())
}
