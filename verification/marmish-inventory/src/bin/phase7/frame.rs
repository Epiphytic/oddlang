use super::*;

fn expand(segments: &Value) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    for segment in array(segments)? {
        let keys = keys(segment)?;
        if keys == set(&["utf8"]) {
            bytes.extend(text(&segment["utf8"])?.as_bytes());
        } else if keys == set(&["hex"]) {
            bytes.extend(hex_decode(text(&segment["hex"])?)?);
        } else if keys == set(&["repeatHex", "count"]) {
            let n = usize::try_from(segment["count"].as_u64().context("invalid repeat count")?)?;
            bytes.extend(hex_decode(text(&segment["repeatHex"])?)?.repeat(n));
        } else {
            bail!("invalid byte segment: {segment}");
        }
    }
    Ok(bytes)
}
pub fn cases(contract: &Value) -> Result<Vec<Value>> {
    array(&contract["cases"])?.iter().map(|case|{
        let expected=&case["expected"];
        let observed=if let Some(bytes)=expected.get("bytes"){
            json!({"bytesHex":hex_encode(&expand(bytes)?)})
        }else{expected.clone()};
        Ok(json!({"id":case["id"],"operation":case["operation"],"inputSha256":digest(&expand(&case["input"])?),"observed":observed}))
    }).collect()
}
pub fn expected_wire(contract: &Value) -> Result<Value> {
    let mut exchanges = vec![];
    let mut ids = Set::new();
    for case in array(&contract["cases"])? {
        if case["operation"] == "encode"
            && let Some(bytes) = case["expected"].get("bytes")
        {
            ensure!(
                ids.insert(text(&case["id"])?.to_owned()),
                "invalid contract exchange identities"
            );
            exchanges.push(json!({"id":case["id"],"rawSha256":digest(&expand(&case["input"])?),"wireSha256":digest(&expand(bytes)?)}));
        }
    }
    ensure!(
        !exchanges.is_empty(),
        "invalid contract exchange identities"
    );
    Ok(json!({
        "cases":cases(contract)?,
        "publicApi":{"original":"tested","candidate":"tested"},
        "directions":[{"writer":"original","reader":"candidate","exchanges":exchanges},
            {"writer":"candidate","reader":"original","exchanges":exchanges}]
    }))
}
pub fn check_wire(contract: &Value, wire: &Value) -> Result<()> {
    ensure!(
        *wire == expected_wire(contract)?,
        "mixed wire role, case identity or observed bytes disagree with contract"
    );
    Ok(())
}
#[path = "../../frame_inputs.rs"]
mod inputs;
use inputs::verify_inputs;
pub fn check_frame(root: &Path, dir: &Path, contract: &Value) -> Result<()> {
    let base = dir.join("frame");
    let observed = base.join("observations");
    let imported = base.join("import");
    let positive = base.join("projection");
    let mutant = base.join("mutant-projection");
    let report = read_json(&observed.join("report.json"))?;
    verify_inputs(&imported, &positive, &mutant)?;
    for (local, producer, names) in [
        (
            &imported,
            "framing",
            ["domain.ttl", "import.lock", "source-map.json"],
        ),
        (
            &positive,
            "framing-projection",
            ["frame.rs", "world.wit", "projection.json"],
        ),
    ] {
        for name in names {
            ensure!(
                read(&local.join(name))?
                    == read(&dir.join("rebuild/first").join(producer).join(name))?,
                "framing observation uses a different projection or input"
            );
        }
    }
    let expected = cases(contract)?;
    let raw = read(&observed.join("original/results.json"))?;
    let original: Value = serde_json::from_slice(&raw)?;
    let contract_sha = digest(&read(&root.join("examples/marmish/observations.json"))?);
    ensure!(
        report["sourceRevision"] == PIN
            && original["sourceRevision"] == PIN
            && report["classification"] == "tested"
            && report["graphSha256"] == digest(&read(&imported.join("domain.ttl"))?)
            && report["importLockSha256"] == digest(&read(&imported.join("import.lock"))?)
            && report["cases"] == expected.len()
            && report["originalSha256"] == digest(&raw)
            && report["contractSha256"] == contract_sha
            && original["contractSha256"] == contract_sha
            && original["cases"] == json!(expected)
            && report["rustc"] == String::from_utf8(read(&observed.join("rustc.txt"))?)?.trim()
            && report["wasmTools"]
                == String::from_utf8(read(&observed.join("wasm-tools.txt"))?)?.trim(),
        "framing report or original observations differ from contract"
    );
    let historical: Value = serde_json::from_str(include_str!("historical-harness.json"))?;
    ensure!(
        report["schemaVersion"] == 1 && valid_hash(&report["hostSha256"], 64),
        "invalid framing report identity"
    );
    if report["harnessSha256"] != historical["files"] {
        ensure!(
            report["harnessSha256"] == inputs::harness(root)?,
            "framing harness identity changed"
        );
    }
    ensure!(
        keys(&report["builds"])? == set(&["regenerated", "mutant"]),
        "missing framing build observations"
    );
    let expected_wit = read(&observed.join("expected.wit"))?;
    for (name, projection) in [("regenerated", &positive), ("mutant", &mutant)] {
        let dest = observed.join(name);
        let result = &report["builds"][name];
        ensure!(
            result["artifacts"]
                == file_hashes(&dest, &["consumer", "component.wasm", "extracted.wit"])?
                && result["projection"]
                    == file_hashes(projection, &["frame.rs", "world.wit", "projection.json"])?
                && array(&result["wasmImports"])?.is_empty()
                && read(&dest.join("extracted.wit"))? == expected_wit
                && keys(&result["results"])? == set(&["native", "wasm"]),
            "framing binary, target or ABI evidence differs"
        );
        let required = if name == "regenerated" {
            json!([])
        } else {
            json!([{"case":"version-before-size","expected":{"error":"InvalidVersion"},"actual":{"error":"SizeLimit"}}])
        };
        for role in ["native", "wasm"] {
            let raw = read(&dest.join(format!("{role}.json")))?;
            let actual: Value = serde_json::from_slice(&raw)?;
            let rows = array(&actual["cases"])?;
            ensure!(
                rows.len() == expected.len(),
                "wrong {name}/{role} contract case count"
            );
            let mut differences = vec![];
            for (want, got) in expected.iter().zip(rows) {
                ensure!(
                    keys(want)? == keys(got)?
                        && ["id", "operation", "inputSha256"]
                            .iter()
                            .all(|k| want[k] == got[k]),
                    "wrong {name}/{role} contract case or input"
                );
                if want["observed"] != got["observed"] {
                    differences.push(json!({"case":want["id"],"expected":want["observed"],"actual":got["observed"]}));
                }
            }
            ensure!(
                required == json!(differences)
                    && actual["differences"] == required
                    && result["results"][role]
                        == json!({"observationsSha256":digest(&raw),"caseCount":expected.len(),"differences":differences}),
                "{name}/{role} observations disagree with independent outcomes"
            );
        }
    }
    Ok(())
}
