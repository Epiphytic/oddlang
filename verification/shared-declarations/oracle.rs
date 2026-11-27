use thumbnail_consumer::{PERIOD, Reading, SensorError};
use thumbnail_consumer::meter::Mode;
#[test]
fn inspect() {
    assert_eq!(PERIOD, 51u16);
    let a: Reading<u32> = Err(SensorError::Label("probe"));
    assert_eq!(a.unwrap_err().to_string(),"sensor {probe}");
    assert_eq!(format!("{:>20}", SensorError::Label("probe")), "sensor {probe}");
    assert_eq!(format!("{:.1}", SensorError::Label("probe")), "sensor {probe}");
    assert_eq!(format!("{:08}", SensorError::Count(42)), "42 cycles");
    assert_eq!(SensorError::Count(u64::MAX).to_string(), "18446744073709551615 cycles");
    assert_eq!(SensorError::Flag(true).to_string(), "true");
    assert_eq!(SensorError::Paused.to_string(), "paused");
    assert_eq!(format!("{:?}", std::mem::discriminant(&SensorError::Paused)), "Discriminant(3)");
    assert_eq!(Mode::Hot as u8,11);
    for b in 0u8..=255 {
        let expected = match b {3=>Ok(Mode::Cold),11=>Ok(Mode::Hot),_=>Err(SensorError::Label("mode code"))};
        assert_eq!(Mode::try_from(b),expected);
    }
    assert_eq!(serde_json::to_string(&Mode::Hot).unwrap(), r#""h.ot""#);
    assert_eq!(serde_json::from_str::<Mode>(r#""COLD""#).unwrap(), Mode::Cold);
    assert!(serde_json::from_str::<Mode>(r#""cold""#).is_err());
}
