// Hash preimage in hex: 050010203007 (domain, three seed bytes, route tag).
#![deny(warnings)]
mod record;
use record::{Position, Reading};
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    Code(&'static str),
}
#[derive(Clone, PartialEq, Eq)]
pub enum Route {
    Local,
}
fn main() {
    let value = Reading {
        count: 10,
        has_flags: true,
        flags: Some(3),
        position: Some(Position {
            degrees: i64::MIN,
            fraction: 999,
        }),
        seed: [16, 32, 48],
        route: Route::Local,
        cluster: vec![42],
    };
    const BYTES: [u8; 25] = [
        0, 10, 1, 1, 3, 1, 128, 0, 0, 0, 0, 0, 0, 0, 3, 231, 16, 32, 48, 7, 0, 0, 0, 1, 42,
    ];
    assert_eq!(value.encode().unwrap(), BYTES);
    assert!(Reading::decode(&BYTES).unwrap() == value);
    assert_eq!(
        value.index(),
        "r-3309b020babbd4f06288dc6f5777abe39dcf47d70cf656536ceb022b1b504156"
    );
    let mut invalid = BYTES.to_vec();
    invalid[15] = 232;
    assert_eq!(
        Reading::decode(&invalid).err(),
        Some(Fault::Code("metadata"))
    );
    invalid.push(7);
    assert_eq!(Reading::decode(&invalid).err(), Some(Fault::Code("extra")));
    assert_eq!(
        Reading::decode(&[0, 11, 9]).err(),
        Some(Fault::Code("counter"))
    );
    assert_eq!(
        Reading::decode(&[0, 10, 9]).err(),
        Some(Fault::Code("bool"))
    );
    assert_eq!(
        Reading::decode(&[0, 10, 1, 9]).err(),
        Some(Fault::Code("option"))
    );
    assert_eq!(
        Reading::decode(&[0, 10, 1, 1]).err(),
        Some(Fault::Code("short"))
    );
    let mut absent = value.clone();
    absent.flags = None;
    assert_eq!(absent.encode().err(), Some(Fault::Code("metadata")));
    absent.has_flags = false;
    absent.position = None;
    assert_eq!(
        absent.encode().unwrap(),
        [0, 10, 0, 0, 0, 16, 32, 48, 7, 0, 0, 0, 1, 42]
    );
    assert!(Reading::decode(&absent.encode().unwrap()).unwrap() == absent);
    let mut bad = value.clone();
    bad.flags = Some(8);
    assert_eq!(bad.validate(), Err(Fault::Code("metadata")));
    assert_eq!(bad.index(), value.index());
    bad.cluster = vec![0];
    assert_eq!(bad.validate(), Err(Fault::Code("cluster")));
}
