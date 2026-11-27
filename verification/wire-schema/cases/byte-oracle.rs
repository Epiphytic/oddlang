#![deny(warnings)]
mod wire;
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Invalid(&'static str),
}
fn main() {
    use wire::Sample;
    let bytes = r#"{"seq":2,"signature":"AP4="}"#;
    let s = serde_json::from_str::<Sample>(bytes).unwrap();
    assert_eq!(s.signature, [0, 254]);
    assert_eq!(serde_json::to_string(&s).unwrap(), bytes);
    assert_eq!(s.check(), Ok(()));
    for path in ["", "/w=="] {
        let bad = serde_json::from_value::<Sample>(serde_json::json!({"seq":0,"signature":path}))
            .unwrap();
        assert_eq!(bad.check(), Err(Failure::Invalid("signature")));
    }
    for (s, want) in [
        ("AA", "wrong encoding"),
        ("AB==", "wrong encoding"),
        ("AAAAAAA=", "decoded length"),
        ("?????????", "too long"),
    ] {
        let err = serde_json::from_value::<Sample>(serde_json::json!({"seq":1,"signature":s}))
            .err()
            .unwrap()
            .to_string();
        assert_eq!(err, want);
    }
}
