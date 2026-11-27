
#[path = "records.rs"] mod public;
#[path = "errors.rs"] mod failure;
fn main() {
    let snapshot = public::Snapshot { primary: "first".into(), secondary: "second".into() };
    assert_eq!(format!("{:?}", snapshot), r#"Snapshot { secondary: "second", primary: "first" }"#);
    let fault = failure::Fault::Empty;
    assert_eq!(format!("{:?}", fault), "Empty");
    assert_eq!(fault.to_string(), "sample missing");
    assert!(std::error::Error::source(&fault).is_none());
    assert_eq!(failure::Fault::Unavailable.to_string(), "sensor unavailable");
}
