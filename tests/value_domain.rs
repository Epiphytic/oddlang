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
        let js = root.join(format!("node-{maximum}"));
        oddlang::value_domain::build_typescript(root, &js).unwrap();
        assert_eq!(
            fs::read(out.join("obligations.json")).unwrap(),
            fs::read(js.join("obligations.json")).unwrap()
        );
        fs::write(js.join("check.mjs"), r#"import assert from 'node:assert/strict';
import {validate, maximum, defaultValue, errorMessages} from './value.mts';
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
        let source = root.join("main.rs");
        fs::write(&source,format!(r#"include!("{maximum}/value.rs");
fn main() {{
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
