//! Compare stdout, stderr and exit status from the reference CLI and both assembled binaries.
use provider_validation::{Result, cli, command, pin, write};
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};
fn observe(exe: &Path, args: &[&str]) -> Result<(Option<i32>, Vec<u8>, Vec<u8>)> {
    let out = Command::new(exe).args(args).stdin(Stdio::null()).output()?;
    Ok((out.status.code(), out.stdout, out.stderr))
}
fn main() -> Result {
    let cli = cli()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../command-surface");
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let original = root.join("original");
    fs::create_dir_all(original.join("src"))?;
    for file in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(fixture.join(file), original.join(file))?;
    }
    fs::copy(
        fixture.join("src/bin/reference.rs"),
        original.join("src/main.rs"),
    )?;
    let compile = |dir: &Path| {
        command(
            Command::new("cargo").env("CARGO_TARGET_DIR", dir.join("target")),
            dir,
            &["build", "--bins", "--offline", "--locked"],
            None,
        )
    };
    compile(&original)?;
    command(
        Command::new(&cli)
            .args(["recover", "cli-surface", "original/src/main.rs"])
            .arg(fixture.join("selection.json"))
            .arg("graph.json"),
        root,
        &[],
        None,
    )?;
    fs::copy(fixture.join("binding.json"), root.join("binding.json"))?;
    fs::copy(fixture.join("Cargo.lock"), root.join("Cargo.lock"))?;
    let mut package = json!({"format":"oddlang-native-package-v1","name":"observatory","edition":"2021","version":"2.1.0",
        "dependencies":{"clap":{"version":"4.6.7","features":["derive"]}},"cargoLock":pin(root,"Cargo.lock")?,
        "binaries":[{"name":"observatory","entry":["run_cli"]}],"components":[{"kind":"cli-surface","module":"command",
        "graph":pin(root,"graph.json")?,"overlay":pin(root,"binding.json")?,"reexports":["run_cli"]}]});
    write(&root.join("package.json"), &package)?;
    let mut workspace = json!({"format":"oddlang-native-workspace-v1","cargoLock":pin(root,"Cargo.lock")?,
        "members":{"observatory":{"manifest":pin(root,"package.json")?}}});
    write(&root.join("workspace.json"), &workspace)?;
    for (kind, spec) in [
        ("native-package", "package.json"),
        ("native-workspace", "workspace.json"),
    ] {
        command(
            &mut Command::new(&cli),
            root,
            &["build", kind, "rust", spec, kind],
            None,
        )?;
        let out = root.join(kind);
        compile(&out)?;
        for argv in [
            &["--help"][..],
            &["--version"],
            &["--absent"],
            &["help", "capture"],
        ] {
            assert_eq!(
                observe(&original.join("target/debug/observatory"), argv)?,
                observe(&out.join("target/debug/observatory"), argv)?
            );
        }
    }
    package["binaries"][0]["entry"] = json!(["missing_entry"]);
    write(&root.join("package.json"), &package)?;
    workspace["members"]["observatory"]["manifest"] = pin(root, "package.json")?;
    write(&root.join("workspace.json"), &workspace)?;
    command(
        &mut Command::new(&cli),
        root,
        &["build", "native-workspace", "rust", "workspace.json", "bad"],
        Some("invalid workspace binary entry binding"),
    )?;
    assert!(!root.join("bad").exists());
    println!(
        "Standalone and composed binaries match the original on stdout, stderr and status; invalid workspace entry rejected"
    );
    Ok(())
}
