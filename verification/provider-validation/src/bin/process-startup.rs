//! Exercise generated binary startup with a typed sibling dependency, before argument parsing.
mod process_limit_support;
use process_limit_support::{build, cargo, pinned, run, success};
use provider_validation::{Result, cli, pin, write};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn seed(
    root: &Path,
    name: &str,
    dependency: bool,
    clap: bool,
    standalone: bool,
) -> Result<PathBuf> {
    let member = root.join(name);
    fs::create_dir_all(member.join("src"))?;
    fs::write(member.join("src/lib.rs"), "")?;
    let mut manifest =
        format!("[package]\nname={name:?}\nversion='0.1.0'\nedition='2021'\n[dependencies]\n");
    if dependency {
        manifest.push_str("starter-lib={path='../starter-lib'}\n");
    }
    if clap {
        manifest.push_str("clap='=4.6.7'\n");
    }
    if standalone {
        manifest.push_str("\n[workspace]\n");
    }
    fs::write(member.join("Cargo.toml"), manifest)?;
    Ok(member)
}
fn main() -> Result {
    let cli = cli()?;
    let scratch = tempfile::tempdir()?;
    let root = scratch.path();
    seed(root, "starter-lib", false, false, false)?;
    seed(root, "field-meter", true, true, false)?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver='2'\nmembers=['starter-lib','field-meter']\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    let mut library = json!({"format":"oddlang-native-package-v1","name":"starter-lib","version":"0.1.0","edition":"2021",
        "components":[{"kind":"process-limit","module":"limits","reexports":["initialize"],
            "graph":pinned(root,"graph.json",&json!({"format":"oddlang-process-limit-v1","id":"urn:meter:initialize","resource":"open-descriptors",
                "platforms":{"linux":{"kind":"no-op"},"macos":{"kind":"no-op"}},"failure":"urn:meter:unavailable"}))?,
            "overlay":pinned(root,"rust.json",&json!({"entry":"initialize","error":{"id":"urn:meter:unavailable","type":"LimitError","variant":"Unavailable","result":"Result","display":"unavailable"}}))?}]});
    let mut product = json!({"format":"oddlang-native-package-v1","name":"field-meter","version":"0.1.0","edition":"2021","dependencies":{"clap":{"version":"4.6.7"}},
        "components":[{"kind":"cli-surface","module":"cli","reexports":["run_cli"],
            "graph":pinned(root,"cli.json",&json!({"profile":"command-surface-v1","package_name":"field-meter","version":"0.1.0","root":{"name":"field-meter","help":"Read a field meter"}}))?,
            "overlay":pinned(root,"cli-rust.json",&json!({"executable":"field-meter","invalid_exit":2,"uncovered_exit":125,"operations":{}}))?}],
        "binaries":[{"name":"field-meter","entry":["run_cli"],"startup":{"dependency":"starter-lib","entry":["initialize"],"exit":4,"stderr":"start rejected"}}]});
    for (member, clap) in [(&mut library, false), (&mut product, true)] {
        let name = member["name"].as_str().unwrap();
        let standalone = seed(&root.join("solo"), name, false, clap, true)?;
        cargo(&standalone, &["generate-lockfile", "--offline"])?;
        let lock = format!("{name}.lock");
        fs::copy(standalone.join("Cargo.lock"), root.join(&lock))?;
        member["cargoLock"] = pin(root, &lock)?;
    }
    let mut workspace = json!({"format":"oddlang-native-workspace-v1","cargoLock":pin(root,"Cargo.lock")?,"members":{
        "starter-lib":{"manifest":pinned(root,"library.json",&library)?},"field-meter":{"dependencies":["starter-lib"]}}});
    let assemble =
        |workspace: &mut Value, product: &Value, name: &str, error: Option<&str>| -> Result {
            workspace["members"]["field-meter"]["manifest"] =
                pinned(root, "product.json", product)?;
            write(&root.join("workspace.json"), workspace)?;
            build(
                &cli,
                root,
                "native-workspace",
                "workspace.json",
                name,
                error,
            )
        };
    assemble(&mut workspace, &product, "product", None)?;
    let output = root.join("product");
    let cargo_args = ["build", "--locked", "--offline", "--bins"];
    cargo(&output, &cargo_args)?;
    let binary = output.join("target/debug/field-meter");
    let got = run(&binary, root, &["--help"])?;
    success(&got)?;
    assert!(String::from_utf8(got.stdout)?.contains("Read a field meter") && got.stderr.is_empty());
    let wrapper = output.join("field-meter/src/bin/field-meter.rs");
    let initial = fs::read_to_string(&wrapper)?;
    assert!(initial.contains("::starter_lib::initialize"));
    fs::write(
        output.join("starter-lib/src/group_0/__projection_0.rs"),
        "pub fn initialize() -> ::std::result::Result<(), ()> { Err(()) }\n",
    )?;
    cargo(&output, &cargo_args)?;
    assert_eq!(initial, fs::read_to_string(wrapper)?);
    let got = run(&binary, root, &["--bad-option"])?;
    assert_eq!(got.status.code(), Some(4));
    assert!(got.stdout.is_empty());
    assert_eq!(got.stderr, b"start rejected\n");
    workspace["members"]["field-meter"]["dependencies"] = json!([]);
    assemble(
        &mut workspace,
        &product,
        "no-edge",
        Some("startup dependency"),
    )?;
    workspace["members"]["field-meter"]["dependencies"] = json!(["starter-lib"]);
    product["binaries"][0]["startup"]["entry"] = json!(["absent"]);
    assemble(
        &mut workspace,
        &product,
        "no-function",
        Some("invalid workspace binary entry binding"),
    )?;
    println!(
        "Sibling startup succeeds before CLI and emits its declared failure before argument parsing"
    );
    Ok(())
}
