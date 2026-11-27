use super::*;
use provider_validation::pin;

pub(super) fn verify(s: &Suite) -> Result {
    let owner = "urn:probe:sample";
    let field = "urn:probe:sample:field:signature";
    let error = "urn:probe:bad";
    let selection = json!({"id":"urn:probe:contract","types":{"Sample":owner},
        "adapters":{"base64_sample":{"kind":"bounded-base64","policy":{
            "maxBytes":4,"encodedLimitMessage":"too long","decodedLimitMessage":"decoded length","syntaxMessage":"wrong encoding"}}}});
    let mut model = recover(s, &s.case("byte-source.rs"), &selection, "observed")?;
    assert_eq!(
        model["graph"]["types"][0]["fields"][1]["type"]["policy"]["maxBytes"],
        4
    );
    model["graph"]["guards"] = json!([
        {"id":"urn:probe:length","owner":owner,"require":{"kind":"count","field":field,"minimum":1,"maximum":4},"error":error},
        {"id":"urn:probe:byte","owner":owner,"require":{"kind":"excludes-byte","field":field,"byte":255},"error":error}]);
    model["binding"]["nativeValidation"] = json!({(owner):{"method":"check","typePath":"crate::Failure","errors":{(error):{"variant":"Invalid","argument":"signature"}}}});
    s.project(&model, "generated", None)?;
    let consumer = s.root.join("consumer");
    fs::create_dir_all(consumer.join("src"))?;
    for name in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(s.fixtures.join("fixture").join(name), consumer.join(name))?;
    }
    fs::copy(
        s.root.join("generated/wire.rs"),
        consumer.join("src/wire.rs"),
    )?;
    let oracle = fs::read_to_string(s.case("byte-oracle.rs"))?;
    fs::write(consumer.join("src/main.rs"), &oracle)?;
    cargo(&consumer, &["run", "--quiet", "--locked", "--offline"])?;
    fs::copy(consumer.join("Cargo.lock"), s.root.join("Cargo.lock"))?;
    let failure = "urn:probe:failure";
    let invalid = "urn:probe:invalid";
    write(
        &s.root.join("errors.json"),
        &json!({"format":"oddlang-shared-declarations-v1","id":"urn:probe:outcome","errors":[
        {"id":failure,"variants":[{"id":invalid,"payload":"text","message":[{"payload":true}]}]}]}),
    )?;
    write(
        &s.root.join("names.json"),
        &json!({"format":"oddlang-shared-rust-v1","names":{(failure):"Failure",(invalid):"Invalid"},
        "variantOrder":{(failure):[invalid]}}),
    )?;
    let manifest = json!({"format":"oddlang-native-package-v1","name":"irrigation-schema-fixture","version":"0.0.0","edition":"2021",
        "dependencies":{"base64":{"version":"0.22.1"},"serde":{"version":"1.0.229","features":["derive"]},"serde_json":{"version":"1.0.154"}},
        "cargoLock":pin(&s.root,"Cargo.lock")?,"components":[
            {"kind":"shared-declarations","module":"outcomes","reexports":["Failure"],"graph":pin(&s.root,"errors.json")?,"overlay":pin(&s.root,"names.json")?},
            {"kind":"wire-schema","module":"readings","visibility":"public","reexports":["Sample"],"graph":pin(&s.root,"graph.json")?,"overlay":pin(&s.root,"rust.json")?}]});
    write(&s.root.join("package.json"), &manifest)?;
    invoke(
        &s.cli,
        &s.root,
        &["build", "native-package", "rust", "package.json"],
        "assembled",
        None,
    )?;
    let package = s.root.join("assembled");
    fs::create_dir(package.join("examples"))?;
    let body = oracle
        .split_once("fn main()")
        .ok_or("missing scalar witness")?
        .1
        .replace(
            "use wire::Sample;",
            "use irrigation_schema_fixture::readings::Sample;",
        );
    fs::write(
        package.join("examples/probe.rs"),
        format!("use irrigation_schema_fixture::Failure;\nfn main(){body}"),
    )?;
    cargo(
        &package,
        &[
            "run",
            "--quiet",
            "--locked",
            "--offline",
            "--example",
            "probe",
        ],
    )?;
    model["binding"]["nativeValidation"][owner]["errors"] = json!({});
    s.project(&model, "bad", Some("invalid native validation binding"))?;
    model["graph"]["guards"][1]["require"]["field"] = json!("urn:probe:sample:field:seq");
    s.project(&model, "bad-type", Some("predicate field type"))
}
