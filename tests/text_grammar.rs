use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const FIXTURE: &str = r#"
pub fn station(value: &str) -> bool {
    let Some(body) = value
        .strip_prefix("sat/")
        .or_else(|| value.strip_prefix("ground/"))
        .or_else(|| value.strip_prefix("ship/"))
    else {
        return false;
    };
    match body.split_once("/probe/") {
        Some((channel, probe)) => octet(channel, true).is_some() && octet(probe, false).is_some(),
        None => octet(body, true).is_some(),
    }
}

fn octet(text: &str, above_zero: bool) -> Option<u8> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let parsed = text.parse::<u8>().ok()?;
    (!above_zero || parsed > 0).then_some(parsed)
}
"#;

fn recover(work: &Path, source: &str, output: &str) -> std::process::Output {
    let data = work.join("source.rs");
    let selection = work.join("selection.json");
    fs::write(&data, source).unwrap();
    fs::write(
        &selection,
        serde_json::to_vec(&json!({
            "id": "urn:telemetry:route",
            "entry": "station",
            "sourceSha256": Sha256::digest(source.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>()
        }))
        .unwrap(),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_oddlang"))
        .args(["recover", "text-grammar", "rust"])
        .arg(data)
        .arg(selection)
        .arg(work.join(output))
        .output()
        .unwrap()
}

fn checked(result: std::process::Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn observe(work: &Path, recovered: &str, cases: &[(&str, bool)]) {
    let generated = work.join("generated");
    checked(
        Command::new(env!("CARGO_BIN_EXE_oddlang"))
            .args(["build", "text-grammar", "rust"])
            .arg(work.join(recovered).join("graph.json"))
            .arg(&generated)
            .output()
            .unwrap(),
    );
    fs::write(
        work.join("probe.rs"),
        format!(
            r#"mod original {{
    include!("source.rs");
}}
mod generated {{
    include!("generated/text.rs");
}}
fn main() {{
    let cases: &[(&str, bool)] = &{cases:?};
    for &(value, expected) in cases {{
        assert_eq!(original::station(value), expected, "{{value}}");
        assert_eq!(generated::accepts(value), expected, "{{value}}");
    }}
}}"#
        ),
    )
    .unwrap();
    let executable = work.join("probe");
    checked(
        Command::new("rustc")
            .args(["--edition=2024"])
            .arg(work.join("probe.rs"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    checked(Command::new(executable).output().unwrap());
}

#[test]
fn recovers_ordered_choices_and_decimal_bounds_and_emits_a_native_grammar() {
    let work = tempfile::tempdir().unwrap();
    checked(recover(work.path(), FIXTURE, "baseline"));
    let graph: Value =
        serde_json::from_slice(&fs::read(work.path().join("baseline/graph.json")).unwrap())
            .unwrap();
    assert_eq!(graph["prefixes"], json!(["sat/", "ground/", "ship/"]));
    assert_eq!(graph["body"]["bits"], 8);
    assert_eq!(graph["body"]["minimum"], 1);
    assert_eq!(graph["suffix"]["delimiter"], "/probe/");
    assert_eq!(graph["suffix"]["body"]["minimum"], 0);
    assert_eq!(graph["consumption"], "entire-input");
    let lock: Value =
        serde_json::from_slice(&fs::read(work.path().join("baseline/recovery.lock")).unwrap())
            .unwrap();
    assert_eq!(lock["sourceScope"], "two-selected-functions");
    assert_eq!(
        lock["unresolved"],
        json!([
            "method and name resolution in the original enclosing module requires a native witness"
        ])
    );
    observe(
        work.path(),
        "baseline",
        &[
            ("sat/0", false),
            ("sat/1", true),
            ("ground/255", true),
            ("ground/256", false),
            ("ship/255/probe/0", true),
            ("ship/255/probe/256", false),
            ("ship/2/probe/1/tail", false),
            ("ground/01", false),
            ("sat/١", false),
            ("air/1", false),
            ("sat/3\n", false),
        ],
    );
}

#[test]
fn accepted_source_changes_move_the_boundary_and_unsupported_guards_fail_closed() {
    let work = tempfile::tempdir().unwrap();
    let changed = FIXTURE
        .replace("ground/", "relay/")
        .replace("/probe/", "/test/")
        .replace("u8", "u16");
    checked(recover(work.path(), &changed, "changed"));
    observe(
        work.path(),
        "changed",
        &[
            ("relay/65535/test/0", true),
            ("relay/65536", false),
            ("ground/7", false),
            ("relay/7/probe/2", false),
            ("sat/256", true),
            ("sat/7/test/01", false),
        ],
    );
    for (unsupported, diagnostic) in [
        (
            FIXTURE.replace("byte.is_ascii_digit()", "byte.is_ascii_hexdigit()"),
            "decimal",
        ),
        (
            FIXTURE.replace("None => octet(body, true)", "None => octet(body, false)"),
            "inconsistent decimal",
        ),
        (format!("use prelude::r#Option;\n{FIXTURE}"), "scope"),
        (
            format!(
                "trait Override {{ fn is_some(self) -> bool; }} impl Override for Option<u8> {{ fn is_some(self) -> bool {{ true }} }}\n{FIXTURE}"
            ),
            "scope",
        ),
        (
            FIXTURE.replace("pub fn station", "#[cfg(unproven)] pub fn station"),
            "attributes",
        ),
    ] {
        let result = recover(work.path(), &unsupported, "unsupported");
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(diagnostic));
        assert!(!work.path().join("unsupported").exists());
    }
}
