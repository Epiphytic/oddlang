use oddlang::value_domain::build;
use std::fs;

const GRAPH: &str = r#"@prefix d: <urn:oddlang:core:> .
<urn:t:pressure> a d:OptionalUnsignedRange ; d:bits 8 ; d:minimum 4 ; d:maximum 12 ; d:defaultValue 8 ; d:rangeError <urn:t:unsafe> .
<urn:t:unsafe> a d:NamedError ; d:message "invalid pressure" .
"#;
const BINDING: &str = r#"{"domain":"urn:t:pressure","typeName":"Pressure","constructor":"measured","accessor":"value","maximumConstant":"MAX","errorType":"PressureError","variants":{"urn:t:unsafe":"OutsideRange"}}"#;

#[test]
fn generated_consumer_tracks_the_graph() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let binding = root.join("binding.json");
    fs::write(&binding, BINDING).unwrap();
    for maximum in [12, 17] {
        fs::write(
            root.join("domain.ttl"),
            GRAPH.replace("maximum 12", &format!("maximum {maximum}")),
        )
        .unwrap();
        let out = root.join(maximum.to_string());
        build(root, &binding, &out).unwrap();
        assert!(build(root, &binding, &out).is_err());
        let js = root.join(format!("node-{maximum}"));
        oddlang::value_domain::build_typescript(root, &js).unwrap();
        assert_eq!(
            fs::read(out.join("obligations.json")).unwrap(),
            fs::read(js.join("obligations.json")).unwrap()
        );
        let cases: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("obligations.json")).unwrap()).unwrap();
        let rows = cases["cases"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        let mut rust_cases = String::new();
        for n in [0, 3, 4, 8, maximum, maximum + 1, 255] {
            let row = rows.iter().find(|r| r["input"] == n.to_string()).unwrap();
            let ok = (4..=maximum).contains(&n);
            assert_eq!(row["rule"], if ok { "inRange" } else { "outOfRange" });
            if ok {
                assert_eq!(row["value"], n.to_string());
                rust_cases.push_str(&format!(
                    "assert_eq!(Pressure::measured(Some({n})).unwrap().value(),Some({n}));"
                ));
            } else {
                assert_eq!(row["error"], "urn:t:unsafe");
                rust_cases.push_str(&format!(
                    "assert_eq!(Pressure::measured(Some({n})),Err(PressureError::OutsideRange));"
                ));
            }
        }
        assert!(rows.contains(&serde_json::json!({"rule":"default","value":"8"})));
        assert!(rows.contains(&serde_json::json!({"rule":"absent","input":null,"value":null})));
        for (folder, artifact) in [(&out, "value.rs"), (&js, "value.mts")] {
            use sha2::{Digest, Sha256};
            let hash = |bytes: Vec<u8>| {
                Sha256::digest(bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            };
            let lock: serde_json::Value =
                serde_json::from_slice(&fs::read(folder.join("projection.lock")).unwrap()).unwrap();
            assert_eq!(
                lock["artifactSha256"],
                hash(fs::read(folder.join(artifact)).unwrap())
            );
            assert_eq!(
                lock["obligationsSha256"],
                hash(fs::read(folder.join("obligations.json")).unwrap())
            );
            if artifact == "value.rs" {
                assert_eq!(lock["bindingSha256"], hash(BINDING.as_bytes().to_vec()));
            } else {
                assert!(lock["bindingSha256"].is_null());
            }
        }
        fs::write(js.join("check.mjs"), r#"import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {validate, maximum, defaultValue, errorMessages} from './value.mts';
for (const row of JSON.parse(readFileSync(new URL('./obligations.json', import.meta.url))).cases) {
    if (row.rule === 'default') assert.equal(defaultValue, BigInt(row.value));
    else {
        const input = row.input === null ? null : BigInt(row.input);
        assert.deepEqual(validate(input), row.error ? {ok:false,error:row.error} : {ok:true,value:input});
    }
}
assert.equal(defaultValue, 8n);
assert.deepEqual(validate(null), {ok:true,value:null});
for (let n=0n; n<=255n; n++) {
    const actual=validate(n);
    assert.deepEqual(actual, n>=4n && n<=maximum ? {ok:true,value:n} : {ok:false,error:'urn:t:unsafe'});
}
assert.equal(errorMessages['urn:t:unsafe'], 'invalid pressure');
"#).unwrap();
        assert!(
            std::process::Command::new("node")
                .arg("--experimental-strip-types")
                .arg(js.join("check.mjs"))
                .status()
                .unwrap()
                .success()
        );
        for (folder, overlay) in [(&out, Some(binding.as_path())), (&js, None)] {
            oddlang::value_domain::verify(root, overlay, folder).unwrap();
            fs::write(folder.join("projection.lock"), b"{}").unwrap();
            assert!(oddlang::value_domain::verify(root, overlay, folder).is_err());
        }
        let source = root.join("main.rs");
        fs::write(&source,format!(r#"include!("{maximum}/value.rs");
fn main() {{
    {rust_cases}
    assert_eq!(Pressure::MAX, {maximum});
    assert_eq!(Pressure::default().value(), Some(8));
    assert_eq!(Pressure::measured(None).unwrap().value(), None);
    for n in 0..=255 {{
        let actual = Pressure::measured(Some(n)).map(|v|v.value());
        let expected = if (4..={maximum}).contains(&n) {{ Ok(Some(n)) }} else {{ Err(PressureError::OutsideRange) }};
        assert_eq!(actual, expected);
    }}
    assert_eq!(PressureError::OutsideRange.to_string(), "invalid pressure");
}}"#)).unwrap();
        let binary = root.join("consumer");
        assert!(
            std::process::Command::new("rustc")
                .args(["--edition=2024", "-Dwarnings"])
                .arg(&source)
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new(binary)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn rejects_unrepresentable_facts_unbound_errors_and_code_in_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let binding = root.join("binding.json");
    let out = root.join("projection");
    for (graph, overlay) in [
        (GRAPH.to_string(), BINDING.replace("measured", "gen")),
        (GRAPH.to_string(), BINDING.replace("measured", "default")),
        (GRAPH.to_string(), BINDING.replace("MAX", "limit")),
        (
            GRAPH.to_string(),
            BINDING.replace("urn:t:unsafe", "urn:t:other"),
        ),
        (
            GRAPH.to_string(),
            BINDING.replace("urn:t:pressure", "urn:t:other"),
        ),
        (
            format!("{GRAPH}\n<urn:t:unknown> d:maximum 4 ."),
            BINDING.to_string(),
        ),
        (
            GRAPH.replace("maximum 12", "maximum 256"),
            BINDING.to_string(),
        ),
        (
            GRAPH.replace("defaultValue 8", "defaultValue 3"),
            BINDING.to_string(),
        ),
        (
            GRAPH.replace("rangeError <urn:t:unsafe>", "rangeError <urn:t:missing>"),
            BINDING.to_string(),
        ),
        (
            GRAPH.to_string(),
            BINDING.replace("PressureError", "X {} fn injected()"),
        ),
    ] {
        fs::write(root.join("domain.ttl"), graph).unwrap();
        fs::write(&binding, overlay).unwrap();
        assert!(build(root, &binding, &out).is_err());
        assert!(!out.exists());
    }
}
