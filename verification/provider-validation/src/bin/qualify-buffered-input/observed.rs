use super::*;
struct Profile {
    name: &'static str,
    graph: Value,
}

fn capture(
    s: &Suite,
    name: &'static str,
    source: &str,
    probe: &str,
    expected: [&str; 3],
) -> Result<Profile> {
    fs::write(s.root.join("src/lib.rs"), format!("{source}{probe}"))?;
    let observed = format!("{name}.observation.json");
    s.shape(&observed)?;
    let trace = format!("{name}.json");
    s.probe(&s.root, Some(&s.root.join(&trace)))?;
    // These trace bytes were collected directly from the independent Rust
    // reference and its three-policy source mutation by the previous runner.
    assert_eq!(
        fs::read(s.root.join(&trace))?,
        fs::read(s.fixture.join(format!("{name}.trace.json")))?
    );
    let args = [
        "recover",
        "observed-buffered-input",
        &observed,
        "selection.json",
        &trace,
    ];
    s.fresh(&args, name, None)?;
    let out = s.root.join(name);
    let graph = read(&out.join("graph.json"))?;
    assert_eq!(graph["capacity"], 8);
    assert_eq!(
        graph["fieldOrder"],
        json!(["input", "storage", "begin", "end"])
    );
    for (key, value) in ["emptyRead", "refillError", "consumedBytes"]
        .into_iter()
        .zip(expected)
    {
        assert_eq!(graph["transitions"][key], value);
    }
    let lock = read(&out.join("recovery.lock"))?;
    assert_eq!(lock["traceSha256"], sha(&s.root.join(&trace))?);
    assert_eq!(lock["observationSha256"], sha(&s.root.join(&observed))?);
    assert_eq!(
        lock["inferred"],
        json!([
            "empty-read policy",
            "refill-error policy",
            "consumed-byte policy"
        ])
    );
    assert!(lock["providerAssumptions"].as_array().unwrap().len() >= 3);
    let repeat = format!("{name}-repeat");
    s.fresh(&args, &repeat, None)?;
    assert_eq!(files(&out)?, files(&s.root.join(repeat))?);
    Ok(Profile { name, graph })
}
fn malformed(s: &Suite, selection: &Value, transitions: &Value) -> Result {
    let observed = "baseline.observation.json";
    let trace_name = "baseline.json";
    s.existing(
        &[
            "recover",
            "observed-buffered-input",
            observed,
            "selection.json",
            trace_name,
        ],
        "baseline",
    )?;
    let mut wrong = selection.clone();
    wrong["transitions"] = transitions.clone();
    write(&s.root.join("wrong-selection.json"), &wrong)?;
    s.fresh(
        &[
            "recover",
            "observed-buffered-input",
            observed,
            "wrong-selection.json",
            trace_name,
        ],
        "authored-during-inference",
        Some("unknown field"),
    )?;
    let trace = read(&s.root.join(trace_name))?;
    for (index, (section, key, value, error)) in [
        ("emptyRead", "payload", json!([0, 0]), "probe payload"),
        (
            "emptyRead",
            "callsAfterEmpty",
            json!(2),
            "empty-read evidence",
        ),
        ("emptyRead", "afterFill", json!([]), "empty-read evidence"),
        ("consume", "requested", json!(4), "partial-consume evidence"),
        (
            "consume",
            "nextReadPrefix",
            json!([0, 0, 0, 1]),
            "retirement evidence",
        ),
        (
            "consume",
            "afterRefill",
            json!([7]),
            "partial-consume evidence",
        ),
        (
            "refillError",
            "returnedError",
            json!(false),
            "refill-error evidence",
        ),
        (
            "refillError",
            "afterRetry",
            trace["refillError"]["payload"].clone(),
            "refill-error evidence",
        ),
        (
            "refillError",
            "callsAfterRetry",
            json!(4),
            "refill-error evidence",
        ),
        (
            "consume",
            "payload",
            json!([1, 2, 3, 4, 5, 6, 7, 8, 9]),
            "probe payload",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut invalid = trace.clone();
        invalid[section][key] = value;
        write(&s.root.join("invalid.json"), &invalid)?;
        s.fresh(
            &[
                "recover",
                "observed-buffered-input",
                observed,
                "selection.json",
                "invalid.json",
            ],
            &format!("rejected-{index}"),
            Some(error),
        )?;
    }
    Ok(())
}
pub(super) fn verify(s: &Suite, reference: &str) -> Result {
    let selection = json!({"declaration":"local::field_scanner::Scanner::{type}","id":"urn:oddlang:example:scanner-buffer",
        "fields":{"input":"source","storage":"block","begin":"begin","end":"end"}});
    write(&s.root.join("selection.json"), &selection)?;
    let mut mutated = reference.to_owned();
    for (constant, old, new) in [
        ("EARLY_EMPTY", "false", "true"),
        ("DISCARD_ON_ERROR", "false", "true"),
        ("ERASE_CONSUMED", "true", "false"),
    ] {
        let before = format!("const {constant}: bool = {old};");
        assert_eq!(mutated.matches(&before).count(), 1);
        mutated = mutated.replace(&before, &format!("const {constant}: bool = {new};"));
    }
    let probe = fs::read_to_string(s.fixture.join("probe.rs"))?;
    let baseline = capture(
        s,
        "baseline",
        reference,
        &probe,
        ["refill-if-exhausted", "reset-begin-retain-end", "erase"],
    )?;
    let changed = capture(
        s,
        "mutated",
        &mutated,
        &probe,
        ["return-zero", "discard-window", "retain"],
    )?;
    fs::remove_file(s.root.join("src/lib.rs"))?;
    for file in fs::read_dir(s.root.join("src"))? {
        assert_ne!(
            file?.path().extension().and_then(|e| e.to_str()),
            Some("rs")
        );
    }
    let shape = |p: &Profile| {
        let mut g = p.graph.clone();
        g.as_object_mut().unwrap().remove("transitions");
        g
    };
    assert_eq!(shape(&baseline), shape(&changed));
    assert_eq!(
        fs::read(s.root.join("baseline/rust.json"))?,
        fs::read(s.root.join("mutated/rust.json"))?
    );
    for profile in [&baseline, &changed] {
        let name = profile.name;
        let out = format!("candidate-{name}");
        let manifest = s.manifest(
            &format!("{name}/graph.json"),
            &format!("{name}/rust.json"),
            "scanner",
        )?;
        s.assemble(&manifest, &out, None)?;
        s.attach(&out, &probe)?;
        let path = s.root.join(format!("{out}.json"));
        s.probe(&s.root.join(out), Some(&path))?;
        assert_eq!(
            fs::read(path)?,
            fs::read(s.root.join(format!("{name}.json")))?
        );
    }
    malformed(s, &selection, &baseline.graph["transitions"])
}
