use super::*;
use serde_json::json;

pub(super) fn verify(suite: &Suite) -> Result {
    suite.project(&suite.fixture, "repeat", None)?;
    for file in ["lines.rs", "projection.lock"] {
        assert_eq!(
            fs::read(suite.root().join("generated").join(file))?,
            fs::read(suite.root().join("repeat").join(file))?
        );
    }
    assert_eq!(
        read(&suite.root().join("generated/projection.lock"))?["sourceSha256"],
        sha(&suite.root().join("generated/lines.rs"))?
    );
    suite.witness(
        "inventory.rs",
        false,
        &[("GENERATED", "generated/lines.rs")],
    )?;
    let numeric = json!([{"kind":"decimal","bits":64,"minimum":0,"capture":"large"}]);
    let text = json!([{"kind":"radix16","digits":2,"decode":false,"capture":"hex"},{"kind":"text","capture":"marker"}]);
    for (name, id, row, reader, record, captures) in [
        (
            "static-record",
            "urn:inventory:largest-count",
            numeric,
            "largest",
            "Maximum",
            json!({"large":"large"}),
        ),
        (
            "text-record",
            "urn:inventory:hex-and-text",
            text,
            "lookup",
            "Option",
            json!({"hex":"hex","marker":"marker"}),
        ),
    ] {
        let fixture = json!({
            "graph":{"format":suite.fixture["graph"]["format"],"id":id,"maxBytes":32,"forbiddenBytes":[9,13],"rows":[row]},
            "overlay":{"format":suite.fixture["overlay"]["format"],"reader":reader,"record":record,"captureNames":captures},
        });
        suite.project(&fixture, name, None)?;
    }
    suite.witness(
        "extras.rs",
        true,
        &[
            ("STATIC", "static-record/lines.rs"),
            ("TEXT", "text-record/lines.rs"),
        ],
    )
}
