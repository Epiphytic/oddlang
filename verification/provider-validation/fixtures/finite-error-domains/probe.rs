extern crate self as sample;
#[derive(Debug)]
pub enum Reading { Missing, Broken }
mod errors;
fn main() {
    use std::error::Error;
    use errors::{Fault, Outcome};
    let err: Fault = std::io::Error::other("private device message").into();
    let result: Outcome<()> = Err(err);
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), "device unavailable");
    assert_eq!(err.source().unwrap().to_string(), "private device message");
    assert!(matches!(err, Fault::Device(_)));
    assert_eq!(Fault::Measurement("42").to_string(), "measurement: 42");
    assert_eq!(Fault::Stop.to_string(), "stop");
    assert_eq!(Fault::Reading(Reading::Missing).to_string(), "reading: ABSENT");
    assert_eq!(Fault::Reading(Reading::Broken).to_string(), "reading: unreadable");
    assert!(Fault::Reading(Reading::Missing).source().is_none());
}
