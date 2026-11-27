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
fn verify_inputs(imported: &Path, positive: &Path, mutant: &Path) -> Result<()> {
    let graph = digest(&read(&imported.join("domain.ttl"))?);
    let lock = read_json(&imported.join("import.lock"))?;
    ensure!(
        lock["graphSha256"] == graph && lock["profile"] == "framing-v1",
        "import lock disagrees with the framing graph"
    );
    for dir in [positive, mutant] {
        let manifest = read_json(&dir.join("projection.json"))?;
        ensure!(
            manifest["version"] == 1 && manifest["files"] == file_hashes(dir, &["frame.rs"])?,
            "projection does not match its pinned source bytes"
        );
        ensure!(
            (dir == positive) == (manifest["graphSha256"] == graph),
            "expected distinct positive and mutant graphs"
        );
    }
    ensure!(
        read(&positive.join("frame.rs"))? != read(&mutant.join("frame.rs"))?,
        "mutant and production code are identical"
    );
    ensure!(
        read(&positive.join("world.wit"))? == read(&mutant.join("world.wit"))?,
        "mutant changed the public component ABI"
    );
    Ok(())
}
fn harness_files(dir: &Path, root: &Path, out: &mut Map) -> Result<()> {
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if path.is_dir() {
            harness_files(&path, root, out)?;
        } else {
            out.insert(
                path.strip_prefix(root)?
                    .to_str()
                    .context("invalid path")?
                    .into(),
                json!(digest(&read(&path)?)),
            );
        }
    }
    Ok(())
}
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
        let mut harness = Map::new();
        harness_files(&root.join("examples/wasm/framing"), root, &mut harness)?;
        for name in [
            "examples/wasm/Cargo.lock",
            "examples/wasm/host/Cargo.toml",
            "examples/wasm/toolchain.lock",
            "scripts/marmish/frame.sh",
            "scripts/marmish/frame_runner.py",
            "scripts/marmish/observations.py",
        ] {
            harness.insert(name.into(), json!(digest(&read(&root.join(name))?)));
        }
        ensure!(
            report["harnessSha256"] == json!(harness),
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
