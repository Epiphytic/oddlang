use super::*;
use serde_json::json;
pub(super) fn verify(s: &Suite, cases: &[Case], base: &str) -> Result {
    let identity = "urn:parcel:chunk";
    let count = "urn:parcel:chunk:capacity";
    let range = "urn:parcel:chunk:end";
    let method = "crate::Parcel::encode";
    let overlay = s.fixtures.join("rust.json");
    for case in cases {
        let source = case.revision(base)?;
        fs::write(&s.source, &source)?;
        let receipt = s.observe(&s.work.join("compiler.json"))?;
        assert_eq!(receipt["format"], "oddlang-rust-frame-guards-v1");
        let Some(expected) = &case.expected else {
            unresolved(&receipt, case.unresolved.as_deref());
            assert!(receipt.get("cases").is_none());
            continue;
        };
        resolved(&receipt);
        if case.name == "baseline" {
            let kinds: Vec<_> = receipt["cases"]["Chunk"]["guards"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| &r["kind"])
                .collect();
            assert_eq!(kinds, [&json!("count"), &json!("end-at-most")]);
        }
        s.select(
            json!({"format":"oddlang-frame-guard-selection-v1","method":method,"cases":{
            (identity):{"count:contents":count,"end-at-most:seq:contents":range}}}),
        )?;
        let graph = s.work.join(format!("{}.graph.json", case.name));
        s.recover(
            "frame-guards",
            &s.fixtures.join("graph.json"),
            &overlay,
            &graph,
            None,
        )?;
        let code = s.build("binary-frame", &graph, &overlay, &s.work.join(&case.name))?;
        s.native(&source, expected)?;
        let preamble = source
            .split_once("pub enum Parcel")
            .ok_or("missing Parcel type")?
            .0;
        s.native(&format!("{preamble}{code}"), expected)?;
        let graph = read(&graph)?;
        let ids: Vec<_> = graph["cases"][identity]["encodeGuards"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            if case.name == "baseline" {
                [count, range]
            } else {
                [range, count]
            }
        );
    }
    Ok(())
}
