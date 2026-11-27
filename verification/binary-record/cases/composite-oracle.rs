// The expected footer is SHA-256 of [74, 0] followed by the payload. Each
// malformed payload below carries its own independently precomputed footer.
#![deny(warnings)]
mod inner;
mod record;
pub use inner::Header;
use record::{Point, Sample};
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Bad(&'static str),
}
fn main() {
    let mut s = Sample {
        header: Header { quantity: 1 },
        first: -1,
        last: 0,
        position: Point { north: -2 },
        cached: None,
        count: 1,
    };
    let bytes = [
        77, 3, 0, 0, 0, 4, 0, 0, 0, 1, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0,
        0, 255, 254, 0, 0, 0, 0, 1, 161, 211, 113, 234, 52, 83, 53, 150, 35, 62, 56, 223, 70, 111,
        127, 179, 221, 156, 225, 49, 218, 82, 36, 65, 119, 62, 18, 1, 202, 115, 104, 216,
    ];
    assert_eq!(s.encode().unwrap(), bytes);
    assert!(Sample::decode(&bytes).unwrap() == s);
    assert_eq!(Sample::decode(&[0; 31]).err(), Some(Failure::Bad("limit")));
    assert_eq!(
        Sample::decode(&[0; 32]).err(),
        Some(Failure::Bad("checksum"))
    );
    assert_eq!(
        Sample::decode(&[
            77, 231, 62, 82, 109, 28, 255, 123, 9, 239, 218, 153, 127, 55, 154, 7, 192, 188, 239,
            170, 173, 64, 192, 185, 4, 107, 175, 85, 54, 204, 231, 177, 44
        ])
        .err(),
        Some(Failure::Bad("short"))
    );
    assert_eq!(
        Sample::decode(&[
            98, 97, 100, 209, 194, 52, 109, 93, 251, 124, 94, 82, 254, 168, 235, 125, 2, 143, 116,
            72, 98, 205, 146, 82, 28, 106, 40, 10, 99, 108, 0, 109, 175, 42, 137
        ])
        .err(),
        Some(Failure::Bad("magic"))
    );
    assert_eq!(
        Sample::decode(&[
            77, 3, 0, 0, 0, 4, 0, 0, 0, 0, 167, 167, 152, 148, 181, 201, 220, 132, 89, 143, 106,
            219, 60, 61, 207, 77, 27, 143, 139, 88, 82, 223, 249, 166, 216, 205, 235, 79, 213, 224,
            243, 95
        ])
        .err(),
        Some(Failure::Bad("value"))
    );
    s.last = i64::MAX;
    assert_eq!(s.validate(), Err(Failure::Bad("period")));
    s.last = 1;
    s.cached = Some(Point { north: 2 });
    assert_eq!(s.validate(), Err(Failure::Bad("point")));
    s.cached = None;
    s.count = 2;
    assert_eq!(s.validate(), Err(Failure::Bad("recount")));
}
