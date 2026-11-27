
use calendar_api::Appointment;

fn main() {
    for input in [r#"{}"#, r#"{"until":null}"#] {
        let open = serde_json::from_str::<Appointment>(input).unwrap();
        let field: Option<u64> = open.until;
        assert_eq!(field, None);
        assert_eq!(serde_json::to_string(&open).unwrap(), r#"{"until":null}"#);
    }
    for value in [0, 9007199254740993_u64, u64::MAX] {
        let text = format!(r#"{{"until":{value}}}"#);
        let appointment = Appointment { until: Some(value) };
        let field: Option<u64> = appointment.until;
        assert_eq!(serde_json::to_string(&appointment).unwrap(), text);
        assert_eq!(serde_json::from_str::<Appointment>(&text).unwrap().until, field);
    }
    for bad in [r#"{"until":18446744073709551616}"#, r#"{"until":-1}"#, r#"{"until":"1"}"#] {
        assert!(serde_json::from_str::<Appointment>(bad).is_err());
    }
}
