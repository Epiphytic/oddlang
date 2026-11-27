#![deny(warnings)]
mod record;
use record::Sample;
#[derive(Clone, PartialEq, Eq)]
pub enum Mode {
    New,
    Closed,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Bad(&'static str),
}
fn main() {
    let mut value = Sample {
        counter: 5,
        fingerprint: [0xf0, 0xff],
        payload: vec![0, 0xff],
        state: Mode::Closed,
        label: "z_".into(),
    };
    const EXPECTED: &[u8] = &[
        84, 0, 0, 0, 0, 5, 240, 255, 0, 0, 0, 2, 0, 255, 4, 0, 0, 0, 2, 122, 95, 0, 0, 0, 1, 33,
    ];
    // The 26-byte record includes a total bound tighter than the sum of all maxima.
    assert_eq!(value.encode().unwrap(), EXPECTED);
    assert!(Sample::decode(EXPECTED).unwrap() == value);
    for prefix in 0..EXPECTED.len() {
        let expected = if prefix < 2 { "header" } else { "short" };
        assert_eq!(
            Sample::decode(&EXPECTED[..prefix]).err(),
            Some(Error::Bad(expected))
        );
    }
    let mut bad = EXPECTED.to_vec();
    bad[5] = 1;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("record")));
    bad[14] = 9;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("tag")));
    bad[14] = 2;
    bad.push(0);
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("extra")));
    bad.push(0);
    bad[0] = 0;
    assert_eq!(Sample::decode(&bad).err(), Some(Error::Bad("decode-size")));
    for (offset, name) in [
        (8, "payload-overflow"),
        (15, "atom-overflow"),
        (21, "trailer-overflow"),
    ] {
        let mut corrupt = EXPECTED[..offset + 4].to_vec();
        corrupt[offset + 3] = 7;
        assert_eq!(Sample::decode(&corrupt).err(), Some(Error::Bad(name)));
    }
    for (offset, name) in [(19, "encoding"), (25, "trailer")] {
        let mut corrupt = EXPECTED.to_vec();
        corrupt[offset] = b'?';
        assert_eq!(Sample::decode(&corrupt).err(), Some(Error::Bad(name)));
    }
    value.counter = 0;
    assert_eq!(value.encode().err(), Some(Error::Bad("record")));
    value.counter = 5;
    value.payload = vec![0; 6];
    assert_eq!(value.validate(), Ok(()));
    assert_eq!(value.encode().err(), Some(Error::Bad("encode-size")));
}
