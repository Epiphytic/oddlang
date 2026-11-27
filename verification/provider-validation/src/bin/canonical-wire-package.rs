//! Verify a source-free weather protocol package and its independent native observer.
use provider_validation::{Result, cli, command, invoke, pin, read, sha, write};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const SAMPLE: &str = "urn:weather:sample";
const SAMPLE_CHECK: &str = "urn:weather:check:sample";
const TEXT_CHECK: &str = "urn:weather:check:text";
const EMPTY_CHECK: &str = "urn:weather:check:empty";
const UNUSED_CHECK: &str = "urn:weather:check:unused";

struct Fixture<'a> {
    cli: &'a Path,
    root: &'a Path,
}
impl Fixture<'_> {
    fn build(&self, manifest: &Value, name: &str, error: Option<&str>) -> Result {
        write(&self.root.join("package.json"), manifest)?;
        invoke(
            self.cli,
            self.root,
            &["build", "native-package", "rust", "package.json"],
            name,
            error,
        )
    }
    fn project(&self, name: &str) -> Result {
        invoke(
            self.cli,
            self.root,
            &[
                "build",
                "wire-protocol",
                "rust",
                "shape.json",
                "shape-rust.json",
                "protocol.json",
                "protocol-rust.json",
            ],
            name,
            None,
        )
    }
    fn update(&self, file: &str, value: &Value) -> Result<Value> {
        write(&self.root.join(file), value)?;
        pin(self.root, file)
    }
}
#[derive(Deserialize)]
struct Case {
    file: String,
    pointer: String,
    value: Value,
    diagnostic: String,
    #[serde(default)]
    insert: bool,
}
fn mutate(doc: &mut Value, case: &Case) -> Result {
    if case.insert {
        let (parent, key) = case
            .pointer
            .rsplit_once('/')
            .ok_or("missing object field")?;
        let obj = doc
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .ok_or("missing parent object")?;
        assert!(obj.insert(key.into(), case.value.clone()).is_none());
    } else {
        *doc.pointer_mut(&case.pointer)
            .ok_or_else(|| format!("unknown mutation path {}", case.pointer))? = case.value.clone();
    }
    Ok(())
}
fn run_observer(root: &Path, fixture: &Path) -> Result {
    let output = root.join("out");
    let target = root.join("target");
    command(
        Command::new("cargo")
            .env("CARGO_ENCODED_RUSTFLAGS", "-Dwarnings")
            .env("CARGO_TARGET_DIR", &target),
        &output,
        &["build", "--locked", "--offline", "--quiet"],
        None,
    )?;
    let debug = target.join("debug");
    let binary = root.join("probe");
    command(
        Command::new("rustc")
            .args(["--edition=2021", "-Dwarnings"])
            .arg(fixture.join("package_fixture.rs"))
            .arg("-L")
            .arg(format!("dependency={}", debug.join("deps").display()))
            .arg("--extern")
            .arg(format!(
                "radio_frame_consumer={}",
                debug.join("libradio_frame_consumer.rlib").display()
            ))
            .arg("-o")
            .arg(&binary),
        root,
        &[],
        None,
    )?;
    command(&mut Command::new(binary), root, &[], None)?;
    Ok(())
}
fn main() -> Result {
    let cli = cli()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let f = Fixture { cli: &cli, root };
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../canonical-wire");
    let weather = fixture.join("weather");
    fs::copy(fixture.join("fixture/Cargo.lock"), root.join("Cargo.lock"))?;
    for name in [
        "shape.json",
        "shape-rust.json",
        "protocol.json",
        "protocol-rust.json",
        "errors.json",
        "errors-rust.json",
    ] {
        fs::copy(weather.join(name), root.join(name))?;
    }
    let shape = json!({"graph":pin(root,"shape.json")?,"overlay":pin(root,"shape-rust.json")?});
    let manifest = json!({"format":"oddlang-native-package-v1","name":"radio-frame-consumer","version":"0.0.0","edition":"2021",
        "dependencies":{"base64":{"version":"0.22.1"},"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.154"},"sha2":{"version":"0.10.9"}},
        "cargoLock":pin(root,"Cargo.lock")?,"components":[
            {"kind":"shared-declarations","module":"domain","reexports":["Failure"],"graph":pin(root,"errors.json")?,"overlay":pin(root,"errors-rust.json")?},
            {"kind":"wire-protocol","module":"wire","visibility":"public","rootReexports":[],"graph":pin(root,"protocol.json")?,"overlay":pin(root,"protocol-rust.json")?,
                "wireShape":shape,"reexports":["FORMAT","Sample","Upload","encode_upload","decode_upload"]}]});
    f.build(&manifest, "out", None)?;
    f.build(&manifest, "alternate", None)?;
    let receipt = read(&root.join("out/package.lock"))?;
    assert_eq!(
        receipt["components"][1]["wireShape"],
        json!({"graphSha256":shape["graph"]["sha256"],"overlaySha256":shape["overlay"]["sha256"]})
    );
    assert_eq!(
        fs::read(root.join("out/package.lock"))?,
        fs::read(root.join("alternate/package.lock"))?
    );
    for (file, expected) in receipt["generatedFiles"]
        .as_object()
        .ok_or("missing generatedFiles")?
    {
        assert_eq!(
            sha(&root.join("out").join(file))?,
            expected.as_str().ok_or("expected file digest")?
        );
    }
    run_observer(root, &fixture)?;
    f.project("wire")?;
    let wire = fs::read_to_string(root.join("wire/wire.rs"))?;
    assert!(!wire.contains("unused rule sentinel"));
    let obligations = read(&root.join("wire/obligations.json"))?;
    let rows = obligations["obligations"]
        .as_array()
        .ok_or("missing obligations")?;
    let owners = rows
        .iter()
        .filter(|v| v["role"] == "record-check")
        .map(|v| {
            (
                v["declaration"].as_str().unwrap(),
                v["owner"].as_str().unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        owners,
        [SAMPLE_CHECK, TEXT_CHECK, EMPTY_CHECK, UNUSED_CHECK]
            .map(|key| (key, SAMPLE))
            .into_iter()
            .collect()
    );
    assert!(
        rows.iter()
            .any(|v| v["recordCheck"] == SAMPLE_CHECK && v["step"]["check"] == TEXT_CHECK)
    );
    assert!(
        rows.iter()
            .any(|v| v["endpoint"] == "urn:weather:send" && v["step"]["check"] == SAMPLE_CHECK)
    );
    let graph = read(&weather.join("protocol.json"))?;
    let binding = read(&weather.join("protocol-rust.json"))?;
    let checks = graph["recordChecks"]
        .as_object()
        .ok_or("missing named checks")?;
    // Preserve a genuinely different key order in the input bytes, independent of JSON map storage.
    let entries = checks
        .iter()
        .rev()
        .map(|(k, v)| format!("{}:{}", json!(k), v))
        .collect::<Vec<_>>()
        .join(",");
    let ordered = serde_json::to_string(&graph)?;
    let reordered = ordered.replacen(
        &graph["recordChecks"].to_string(),
        &format!("{{{entries}}}"),
        1,
    );
    assert_ne!(ordered, reordered);
    assert_eq!(serde_json::from_str::<Value>(&reordered)?, graph);
    fs::write(root.join("protocol.json"), &reordered)?;
    f.project("reordered")?;
    for file in ["wire.rs", "obligations.json"] {
        assert_eq!(
            fs::read(root.join("wire").join(file))?,
            fs::read(root.join("reordered").join(file))?
        );
    }
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(weather.join("rejections.json"))?)?;
    for (i, case) in cases.iter().enumerate() {
        let mut graph = graph.clone();
        let mut binding = binding.clone();
        let doc = match case.file.as_str() {
            "protocol.json" => &mut graph,
            "protocol-rust.json" => &mut binding,
            _ => return Err("unexpected mutation source".into()),
        };
        mutate(doc, case)?;
        let mut m = manifest.clone();
        m["components"][1]["graph"] = f.update("protocol.json", &graph)?;
        m["components"][1]["overlay"] = f.update("protocol-rust.json", &binding)?;
        f.build(&m, &format!("invalid-guard-{i}"), Some(&case.diagnostic))?;
    }
    let mut data = read(&weather.join("shape.json"))?;
    let mut names = read(&weather.join("shape-rust.json"))?;
    let external = "urn:weather:external-sampler";
    data["types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"external","id":external}));
    names["externals"][external] = json!("crate::Sampler");
    let mut bad = manifest.clone();
    let protocol = &mut bad["components"][1];
    protocol["wireShape"]["graph"] = f.update("shape.json", &data)?;
    protocol["wireShape"]["overlay"] = f.update("shape-rust.json", &names)?;
    let mut wrong_owner = graph.clone();
    wrong_owner["recordChecks"][UNUSED_CHECK]["owner"] = json!(external);
    protocol["graph"] = f.update("protocol.json", &wrong_owner)?;
    protocol["overlay"] = f.update("protocol-rust.json", &binding)?;
    f.build(
        &bad,
        "external-check-owner",
        Some("a shared check requires a named record"),
    )?;
    for name in ["shape.json", "shape-rust.json", "protocol-rust.json"] {
        fs::copy(weather.join(name), root.join(name))?;
    }
    let entry = format!(
        "{}:{}",
        json!(EMPTY_CHECK),
        graph["recordChecks"][EMPTY_CHECK]
    );
    let duplicate = ordered.replacen(&entry, &format!("{entry},{entry}"), 1);
    assert_ne!(ordered, duplicate);
    fs::write(root.join("protocol.json"), duplicate)?;
    let mut m = manifest.clone();
    m["components"][1]["graph"] = pin(root, "protocol.json")?;
    f.build(&m, "duplicate-check-key", Some("duplicate"))?;
    fs::copy(weather.join("protocol.json"), root.join("protocol.json"))?;
    let mut m = manifest.clone();
    m["components"][1]
        .as_object_mut()
        .unwrap()
        .remove("wireShape");
    f.build(
        &m,
        "missing",
        Some("wire-protocol component requires wireShape"),
    )?;
    let mut m = manifest.clone();
    m["components"][0]["wireShape"] = shape.clone();
    f.build(&m, "unused", Some("wireShape is only valid"))?;
    for key in ["graph", "overlay"] {
        let mut m = manifest.clone();
        m["components"][1]["wireShape"][key]["sha256"] = json!("0".repeat(64));
        f.build(&m, &format!("bad-{key}"), Some("input digest mismatch"))?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"generatedFiles":receipt["generatedFiles"],"wireRustSha256":sha(&root.join("wire/wire.rs"))?,
        "obligationsSha256":sha(&root.join("wire/obligations.json"))?,"obligations":rows.len(),"invalidGuardCases":cases.len(),"additionalRejectionCases":6})
        )?
    );
    Ok(())
}
