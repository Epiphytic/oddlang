use parking_api::{Stay, Zone};
#[test]
fn source_shape() {
    let s = Stay {
        departure: Some(u64::MAX),
        zone: Zone::North,
    };
    let end: Option<u64> = s.departure;
    assert_eq!(end, Some(u64::MAX));
}
