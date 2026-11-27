use super::*;
pub(super) fn verify(
    s: &Suite,
    raw: &Value,
    binding: &Value,
    flat: &Path,
    positive: &Path,
) -> Result {
    let inputs = s.root.join("package-input");
    fs::create_dir(&inputs)?;
    fs::copy(s.fixture.join("Cargo.lock"), inputs.join("Cargo.lock"))?;
    fs::copy(s.fixture.join("graph.json"), inputs.join("journal.json"))?;
    fs::copy(flat, inputs.join("journal-rust.json"))?;
    let error = format!("{PREFIX}shared-error");
    let errors = raw["errors"].as_array().unwrap();
    let variants:Vec<_>=errors.iter().map(|id|json!({"id":id,"payload":"none","message":[{"text":id.as_str().unwrap().strip_prefix(PREFIX).unwrap()}]})).collect();
    write(
        &inputs.join("shared.json"),
        &json!({"format":"oddlang-shared-declarations-v1","id":format!("{PREFIX}declarations"),
        "errors":[{"id":error,"variants":variants}]}),
    )?;
    let mut names = binding["errorVariants"].clone();
    names[&error] = json!("QueueError");
    write(
        &inputs.join("shared-rust.json"),
        &json!({"format":"oddlang-shared-rust-v1","names":names,"constantTypes":{},"variantOrder":{(error):errors}}),
    )?;
    let package = json!({"format":"oddlang-native-package-v1","name":"telemetry-journal-fixture","version":"0.1.0","edition":"2021",
        "dependencies":{"sha2":{"version":"0.10.9"}},"cargoLock":pin(&inputs,"Cargo.lock")?,"components":[
        {"kind":"shared-declarations","module":"queue","visibility":"public","graph":pin(&inputs,"shared.json")?,"overlay":pin(&inputs,"shared-rust.json")?,
            "reexports":["QueueError"],"rootReexports":["QueueError"]},
        {"kind":"sequence-journal","module":"queue","visibility":"public","graph":pin(&inputs,"journal.json")?,"overlay":pin(&inputs,"journal-rust.json")?,
            "reexports":["BoundaryQueue","TelemetryQueue"],"rootReexports":[]}]});
    write(&inputs.join("package.json"), &package)?;
    let out = s.root.join("assembled");
    command(
        &mut Command::new(&s.cli),
        &s.root,
        &[
            "build",
            "native-package",
            "rust",
            "package-input/package.json",
            "assembled",
        ],
        None,
    )?;
    assert_eq!(
        read(&out.join("package.lock"))?["components"][1]["projection"]["obligationsSha256"],
        sha(&positive.join("obligations.json"))?
    );
    s.append_lexical(&out.join("src/group_0/__projection_1.rs"))?;
    fs::create_dir(out.join("tests"))?;
    let caller = fs::read_to_string(s.fixture.join("src/main.rs"))?;
    let body = caller
        .split_once("fn main() {")
        .ok_or("missing independent queue caller")?
        .1
        .replace(
            "use journal::{BoundaryQueue, TelemetryQueue};",
            "use telemetry_journal_fixture::queue::{BoundaryQueue, TelemetryQueue};",
        );
    fs::write(
        out.join("tests/queue.rs"),
        format!("use telemetry_journal_fixture::QueueError;\n#[test]\nfn queue() {{{body}"),
    )?;
    let tests = s.native(&out, "test", true, None)?;
    assert!(tests.contains("1 passed; 0 failed") && tests.contains("2 passed; 0 failed"));
    Ok(())
}
