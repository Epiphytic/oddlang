#[allow(dead_code)]
#[path = "support/http_provider.rs"]
mod live;

use serde_json::json;
use std::{fs, path::Path, process::Command};

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for result in fs::read_dir(source).unwrap() {
        let file = result.unwrap();
        let target = destination.join(file.file_name());
        if file.file_type().unwrap().is_dir() {
            copy_dir(&file.path(), &target);
        } else {
            fs::copy(file.path(), target).unwrap();
        }
    }
}

fn check(mut command: Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn one_pin_builds_both_hybrid_entries_against_live_versions() {
    let root = std::env::current_dir().unwrap();
    let example = root.join("examples/http/hybrid");
    assert!(
        example.join("ssr/server.ts").is_file(),
        "missing TypeScript SSR entry"
    );
    assert!(
        example.join("web/main.tsx").is_file(),
        "missing React entry"
    );
    let workspace = tempfile::tempdir().unwrap();
    for file in ["package.json", "package-lock.json"] {
        fs::copy(example.join(file), workspace.path().join(file)).unwrap();
    }
    let mut install = Command::new("npm");
    install.current_dir(workspace.path()).args([
        "ci",
        "--offline",
        "--ignore-scripts",
        "--no-audit",
    ]);
    check(install);

    let contract = root.join("examples/http/contract");
    let project = contract.join("v1/project.ttl");
    let pin = workspace.path().join("workspace.lock.json");
    let cache = root.join("contracts/cache");
    let mut deployed = Vec::new();
    let fixture =
        json!([{"id":"A","owner":"Alice","balance":"10"},{"id":"B","owner":"Bob","balance":"7"}]);

    for (version, wire) in [("v1", "balance"), ("v2", "balanceMinor")] {
        fs::copy(contract.join(version).join("workspace.lock.json"), &pin).unwrap();
        let build = live::ProviderBuild::generate(
            &project,
            &pin,
            &cache,
            &root.join("examples/http/provider/overlay.json"),
        );
        let binary = build.build();
        let provider = live::Provider::launch(&binary, &fixture);
        let app = workspace.path().join(version);
        for file in ["ssr/server.ts", "web/index.html", "web/main.tsx"] {
            let output = app.join(file);
            fs::create_dir_all(output.parent().unwrap()).unwrap();
            fs::copy(example.join(file), output).unwrap();
        }
        let mut generate = Command::new(env!("CARGO_BIN_EXE_oddlang"));
        generate.args(["http", "generate"]).args([
            &project,
            &pin,
            &cache,
            &root.join("examples/http/clients/typescript/overlay.json"),
            &app.join("generated"),
        ]);
        check(generate);
        let vite = workspace.path().join("node_modules/.bin/vite");
        let mut web = Command::new(&vite);
        web.current_dir(&app)
            .args(["build", "web", "--outDir", "../dist/web"]);
        check(web);
        let mut ssr = Command::new(&vite);
        ssr.current_dir(&app)
            .args(["build", "--ssr", "ssr/server.ts", "--outDir", "dist/ssr"]);
        check(ssr);

        let mut browser = Command::new("node");
        browser
            .arg(root.join("tests/support/http-hybrid.cjs"))
            .arg(app.join("dist/ssr/server.js"))
            .arg(workspace.path().join("node_modules/playwright"))
            .arg(format!("http://{}", provider.addr))
            .arg(wire);
        check(browser);
        let target = Path::new("target/http").join(version);
        fs::create_dir_all(&target).unwrap();
        fs::copy(&binary, target.join("provider")).unwrap();
        fs::copy(app.join("generated/client.ts"), target.join("client.ts")).unwrap();
        copy_dir(&app.join("dist"), &target.join("dist"));
        fs::write(target.join("package.json"), r#"{"type":"module"}"#).unwrap();
        fs::write(
            target.join("fixture.json"),
            serde_json::to_vec_pretty(&fixture).unwrap(),
        )
        .unwrap();
        deployed.push((build, provider));
    }

    let v1 = &deployed[0].1;
    let outside = tempfile::tempdir().unwrap();
    let go = root.join("examples/http/clients/go");
    for file in [
        "go.mod",
        "client.go",
        "peer_test.go",
        "claim.json",
        "target.lock.json",
    ] {
        fs::copy(go.join(file), outside.path().join(file)).unwrap();
    }
    let evidence = outside.path().join("live.json");
    let bundle =
        cache.join("http/485a90a817494ba1aa137cb9777c3d07c338735720df475c1c113b8fe0217dc4/v1");
    let mut independent = Command::new("go");
    independent
        .current_dir(outside.path())
        .env("GOWORK", "off")
        .env("GOPROXY", "off")
        .env("GOTOOLCHAIN", "local")
        .env_remove("PIN")
        .env("BUNDLE", bundle)
        .env("CLAIM", outside.path().join("claim.json"))
        .env("EVIDENCE", &evidence)
        .env("SMOKE", format!("http://{}", v1.addr))
        .args(["test", "-count=1", "-run", "^TestConformance$"]);
    check(independent);
    let observation: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence).unwrap()).unwrap();
    assert_eq!(
        observation,
        json!({"kind":"success","value":{"id":"A","owner":"Alice","balance":"7"}})
    );
}
