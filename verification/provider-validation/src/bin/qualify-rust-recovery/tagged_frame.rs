use super::*;
use serde_json::json;
fn case_named<'a>(receipt: &'a Value, name: &str) -> &'a Value {
    receipt["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["variant"] == name)
        .expect("missing observed case")
}
fn fields(row: &Value) -> Vec<Option<&str>> {
    row["writes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|slot| slot["field"].as_str())
        .collect()
}
pub(super) fn verify(s: &Suite, cases: &[Case], base: &str) -> Result {
    let support = fs::read_to_string(s.fixtures.join("support.rs"))?;
    let overlay = s.fixtures.join("rust.json");
    for case in cases {
        let source = case.revision(base)?;
        fs::write(&s.source, &source)?;
        let receipt = s.observe(&s.work.join("compiler.json"))?;
        assert_eq!(receipt["format"], "oddlang-rust-tagged-frame-v1");
        let Some(expected) = &case.expected else {
            let row = case
                .variant
                .as_ref()
                .map_or(&receipt, |v| case_named(&receipt, v));
            unresolved(row, case.unresolved.as_deref());
            assert!(row.get("writes").is_none());
            continue;
        };
        resolved(&receipt);
        let key = case_named(&receipt, "Key");
        if case.name == "baseline" {
            assert_eq!(key["tag"], 31);
            assert_eq!(fields(key), [Some("first"), Some("second")]);
            let reading = case_named(&receipt, "Reading");
            assert_eq!(reading["tag"], 32);
            assert_eq!(fields(reading), [Some("value"), Some("footer"), None]);
            assert_eq!(
                reading["writes"].as_array().unwrap().last().unwrap()["literal"],
                84
            );
        } else {
            assert_eq!(key["tag"], 34);
            assert_eq!(fields(key), [Some("second"), Some("first")]);
        }
        s.select(
            json!({"format":"oddlang-tagged-frame-selection-v1","method":s.method,
            "tupleSlots":{},"fragmentHelpers":{},"acceptedUnresolved":{}}),
        )?;
        let out = s.work.join(format!("{}-recovery", case.name));
        s.recover(
            "tagged-frame",
            &s.fixtures.join("template.json"),
            &overlay,
            &out,
            None,
        )?;
        assert_eq!(
            read(&out.join("obligations.json"))?["unresolved"],
            json!({})
        );
        let code = s.build(
            "binary-frame",
            &out.join("graph.json"),
            &overlay,
            &s.work.join(&case.name),
        )?;
        s.native(&source, expected)?;
        s.native(&format!("{support}{code}"), expected)?;
    }
    assert_ne!(
        fs::read(s.work.join("baseline-recovery/graph.json"))?,
        fs::read(s.work.join("mutant-recovery/graph.json"))?
    );
    Ok(())
}
