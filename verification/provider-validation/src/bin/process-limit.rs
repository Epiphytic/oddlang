//! Exercise inherited descriptor limits, idempotence, open descriptors and named errors.
mod process_limit_support;
use process_limit_support::{build, cargo, pinned, run, success};
use provider_validation::{Result, cli, pin, write};
use serde_json::json;
use std::fs;

fn main() -> Result {
    let cli = cli()?;
    let scratch = tempfile::tempdir()?;
    let root = scratch.path();
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='water-monitor'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nlibc='=0.2.189'\n",
    )?;
    cargo(root, &["generate-lockfile", "--offline"])?;
    let mut graph = json!({
        "format":"oddlang-process-limit-v1","id":"urn:water:descriptor-budget","resource":"open-descriptors",
        "platforms":{"linux":{"kind":"no-op"},"macos":{"kind":"lower-soft","ceiling":384}},
        "failure":"urn:water:budget-unavailable"
    });
    let mut overlay = json!({"entry":"initialize_budget","error":{
        "type":"BudgetError","variant":"Unavailable","result":"Result","id":"urn:water:budget-unavailable","display":"budget unavailable"
    }});
    let mut package = json!({"format":"oddlang-native-package-v1","name":"water-monitor","version":"0.1.0","edition":"2021",
        "cargoLock":pin(root,"Cargo.lock")?, "dependencies":{"libc":{"version":"0.2.189"}},
        "components":[{"kind":"process-limit","module":"limits","reexports":["initialize_budget","BudgetError","Result"],
            "graph":pinned(root,"graph.json",&graph)?,"overlay":pinned(root,"rust.json",&overlay)?}]
    });
    write(&root.join("package.json"), &package)?;
    build(
        &cli,
        root,
        "native-package",
        "package.json",
        "product",
        None,
    )?;
    let product = root.join("product");
    fs::create_dir(product.join("src/bin"))?;
    fs::write(
        product.join("src/bin/inspect.rs"),
        include_str!("../../fixtures/process-limits/inspect.rs"),
    )?;
    cargo(
        &product,
        &["build", "--locked", "--offline", "--bin", "inspect"],
    )?;
    for soft in [383, 384, 385] {
        let got = run(
            &product.join("target/debug/inspect"),
            root,
            &[&soft.to_string()],
        )?;
        success(&got)?;
        let expected = if cfg!(target_os = "macos") {
            soft.min(384)
        } else {
            soft
        };
        assert_eq!(got.stdout, format!("{soft} {expected}\n").as_bytes());
        assert!(got.stderr.is_empty(), "{got:?}");
    }
    let reject = |package: &serde_json::Value, error: &str| -> Result {
        write(&root.join("package.json"), package)?;
        build(
            &cli,
            root,
            "native-package",
            "package.json",
            "invalid",
            Some(error),
        )
    };
    graph["platforms"]["linux"]["ceiling"] = json!(384);
    package["components"][0]["graph"] = pinned(root, "graph.json", &graph)?;
    reject(&package, "unknown field")?;
    graph["platforms"]["linux"]
        .as_object_mut()
        .unwrap()
        .remove("ceiling");
    graph["platforms"]["macos"]["ceiling"] = json!(0);
    package["components"][0]["graph"] = pinned(root, "graph.json", &graph)?;
    reject(&package, "ceiling")?;
    graph["platforms"]["macos"]["ceiling"] = json!(384);
    package["components"][0]["graph"] = pinned(root, "graph.json", &graph)?;
    overlay["error"]["id"] = json!("urn:water:other");
    package["components"][0]["overlay"] = pinned(root, "rust.json", &overlay)?;
    reject(&package, "failure identity")?;
    println!(
        "Portable descriptor budget: low/equal/high child cases and invalid graph/binding passed"
    );
    Ok(())
}
