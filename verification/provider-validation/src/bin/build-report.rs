//! Exercise the generated provenance command against independent ordered wire observations.
use provider_validation::{Result, cli, command, invoke, pin, read, sha, write};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
const OP: &str = "urn:observatory:provenance";
fn pinned(root: &Path, name: &str, doc: &Value) -> Result<Value> {
    write(&root.join(name), doc)?;
    pin(root, name)
}
struct Fixture {
    root: PathBuf,
    cli: PathBuf,
}
impl Fixture {
    fn build(
        &self,
        pkg: &mut Value,
        report: &Value,
        name: &str,
        error: Option<&str>,
    ) -> Result<PathBuf> {
        pkg["components"][0]["graph"] = pinned(&self.root, "report.json", report)?;
        write(&self.root.join("package.json"), pkg)?;
        invoke(
            &self.cli,
            &self.root,
            &["build", "native-package", "rust", "package.json"],
            name,
            error,
        )?;
        let out = self.root.join(name);
        let target = self.root.join("target");
        if error.is_none() {
            let lock = read(&out.join("package.lock"))?;
            assert_eq!(
                lock["generatedFiles"]["build.rs"],
                sha(&out.join("build.rs"))?
            );
            command(
                Command::new("cargo").env("CARGO_TARGET_DIR", &target),
                &out,
                &["build", "--locked", "--offline"],
                None,
            )?;
        }
        Ok(target.join("debug/observatory"))
    }
    fn run(&self, exe: &Path, args: &[&str]) -> Result<Output> {
        Ok(Command::new(exe)
            .arg("about")
            .args(args)
            .current_dir(&self.root)
            .env("TARGET", "wrong")
            .env("CARGO_PKG_VERSION", "wrong")
            .output()?)
    }
}
fn main() -> Result {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let f = Fixture {
        root: root.into(),
        cli: cli()?,
    };
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    let cargo = root.join("Cargo.toml");
    let cargo_toml = "[package]\nname='observatory'\nversion='2.1.0'\nedition='2021'\n[dependencies]\nclap='=4.6.7'\nserde_json='=1.0.154'\n";
    fs::write(&cargo, cargo_toml)?;
    command(
        &mut Command::new("cargo"),
        root,
        &["generate-lockfile", "--offline"],
        None,
    )?;
    let revision = "af".repeat(20);
    let field = |key: &str, source: Value| json!({"id":format!("urn:obs:field:{key}"),"wire":key,"source":source});
    let graph = json!({"format":"oddlang-build-report-v1","id":OP,"fields":[
        field("edition",json!({"kind":"unsigned","bits":8,"value":2})),field("api",json!({"kind":"package-version"})),field("target",json!({"kind":"build-target"})),
        field("source",json!({"kind":"revision","value":revision})),field("formats",json!({"kind":"unsigned-list","bits":32,"length":3,"value":[0,2,65536]}))]});
    let mut surface = json!({"profile":"command-surface-v1","package_name":"observatory","version":"2.1.0","root":{"name":"observatory","subcommands":[{"name":"about"}]}});
    let adapter = json!({"executable":"observatory","invalid_exit":2,"uncovered_exit":125,"operations":{"about":OP},"handlers":{(OP):["report","show"]}});
    let mut pkg = json!({"format":"oddlang-native-package-v1","name":"observatory","version":"2.1.0","edition":"2021",
        "cargoLock":pin(root,"Cargo.lock")?,"dependencies":{"clap":{"version":"4.6.7"},"serde_json":{"version":"1.0.154"}},
        "components":[{"kind":"build-report","module":"report","reexports":["show"],"overlay":pinned(root,"native.json",&json!({"entry":"show"}))?},
        {"kind":"cli-surface","module":"cli","reexports":["run_cli"],"graph":pinned(root,"cli.json",&surface)?,"overlay":pinned(root,"cli-rust.json",&adapter)?}],
        "binaries":[{"name":"observatory","entry":["run_cli"]}]});
    let version = String::from_utf8(command(&mut Command::new("rustc"), root, &["-vV"], None)?)?;
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("missing host triple")?;
    let expect = |key: &str, revision: &str| {
        format!(
            "{{{}:2,\"api\":\"2.1.0\",\"target\":{},\"source\":{},\"formats\":[0,2,65536]}}\n",
            json!(key),
            json!(host),
            json!(revision)
        )
    };
    let ordered = |out: Output, text: String| {
        assert!(out.status.success() && out.stderr.is_empty(), "{out:?}");
        assert_eq!(out.stdout, text.as_bytes());
    };
    let binary = f.build(&mut pkg, &graph, "product", None)?;
    ordered(f.run(&binary, &[])?, expect("edition", &revision));
    let mut altered = graph.clone();
    altered["fields"][0]["wire"] = json!("wire\"key");
    altered["fields"][3]["source"]["value"] = json!("12".repeat(20));
    let binary = f.build(&mut pkg, &altered, "altered", None)?;
    ordered(f.run(&binary, &[])?, expect("wire\"key", &"12".repeat(20)));
    for (key, value, error) in [
        ("id", graph["fields"][1]["id"].clone(), "duplicate"),
        ("wire", json!("api"), "duplicate"),
        (
            "source",
            json!({"kind":"revision","value":"main"}),
            "revision",
        ),
        (
            "source",
            json!({"kind":"unsigned","bits":8,"value":256}),
            "range",
        ),
        (
            "source",
            json!({"kind":"unsigned","bits":0,"value":0}),
            "width",
        ),
        (
            "source",
            json!({"kind":"unsigned-list","bits":32,"length":2,"value":[1]}),
            "length",
        ),
        (
            "source",
            json!({"kind":"unsigned-list","bits":32,"length":1,"value":[4294967296u64]}),
            "range",
        ),
    ] {
        let mut wrong = graph.clone();
        wrong["fields"][0][key] = value;
        f.build(&mut pkg, &wrong, "invalid", Some(error))?;
    }
    surface["root"]["arguments"] = json!([{"id":"hold","long":"hold","global":true},{"id":"delay","long":"delay","kind":"string","global":true},
        {"id":"mode","long":"mode","kind":"string","global":true},{"id":"path","long":"path","kind":"path","global":true}]);
    pkg["components"][1]["graph"] = pinned(root, "cli.json", &surface)?;
    let mut guarded = graph.clone();
    guarded["guards"] = json!([
        {"condition":"flag_false","argument":"hold","rejected":{"exit":4,"stderr":"hold"}},
        {"condition":"absent","argument":"delay","rejected":{"exit":5,"stderr":"delay"}},
        {"condition":"one_of","argument":"mode","values":["safe"],"rejected":{"exit":6,"stderr":"mode"}},
        {"condition":"path_present","argument":"path","rejected":{"exit":7,"stderr":"path"}}]);
    let binary = f.build(&mut pkg, &guarded, "guarded", None)?;
    for (args, code, msg) in [
        (&["--hold", "--delay", "1", "--mode", "bad"][..], 4, "hold"),
        (&["--delay", "1", "--mode", "bad"][..], 5, "delay"),
        (&["--mode", "bad"][..], 6, "mode"),
        (&[][..], 7, "path"),
    ] {
        let out = f.run(&binary, args)?;
        assert_eq!(out.status.code(), Some(code));
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, format!("{msg}\n").as_bytes());
    }
    let got = f.run(&binary, &["--mode", "safe", "--path", "x"])?;
    assert!(got.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&got.stdout)?,
        serde_json::from_str::<Value>(&expect("edition", &revision))?
    );
    guarded["id"] = json!("urn:observatory:other");
    f.build(
        &mut pkg,
        &guarded,
        "wrong-operation",
        Some("operation identity differs"),
    )?;
    fs::write(&cargo, cargo_toml.replace("2.1.0", "2.1.1"))?;
    command(
        &mut Command::new("cargo"),
        root,
        &["generate-lockfile", "--offline"],
        None,
    )?;
    pkg["version"] = json!("2.1.1");
    pkg["cargoLock"] = pin(root, "Cargo.lock")?;
    surface["version"] = json!("2.1.1");
    pkg["components"][1]["graph"] = pinned(root, "cli.json", &surface)?;
    let binary = f.build(&mut pkg, &graph, "new-package-version", None)?;
    let out = f.run(&binary, &[])?;
    assert!(out.status.success() && out.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout)?["api"],
        "2.1.1"
    );
    println!(
        "Build report matches independent ordered JSON, native build facts and checked package inputs."
    );
    Ok(())
}
