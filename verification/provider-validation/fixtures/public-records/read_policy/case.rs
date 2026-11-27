
#[test]
fn wire_policy_examples() {
    use crate::{Sample, State};

    let unknown = r#"{"index":4,"state":"collected","extra":9}"#;
    let known = r#"{"state":"collected","index":4}"#;
    let array = r#"[4,null,"collected"]"#;
    let expected = Sample { index: 4, label: None, state: State::Collected };
    assert_eq!(::serde_json::from_str::<Sample>(known).unwrap(), expected);
    assert_eq!(::serde_json::from_str::<Sample>(array).unwrap(), expected);
    assert_eq!(::serde_json::to_string(&expected.clone()).unwrap(), r#"{"index":4,"label":null,"state":"collected"}"#);
    assert!(::serde_json::from_str::<Sample>(r#"{"index":4,"index":5,"state":"collected"}"#).is_err());
    if CLOSED {
        assert!(::serde_json::from_str::<Sample>(unknown).unwrap_err().to_string().starts_with("unknown field"));
    } else {
        assert_eq!(::serde_json::from_str::<Sample>(unknown).unwrap(), expected);
    }
}
