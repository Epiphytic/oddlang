mod generated;

use generated::{FORMAT, PROTOCOL, Readback, Transmit, decode_message, decode_reply, encode_message, encode_reply, raw_digest, transmit_digest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure { Oversize, Malformed(&'static str), Range, Early }

fn main() {
    use Failure::{Malformed, Oversize, Range};
    assert_eq!((PROTOCOL, FORMAT), ("radio", 7));
    let packet = Transmit { sequence: 5, gain: 13 };
    let valid = br#"{"protocol":"radio","version":7,"sequence":5,"message":{"dir":"send","payload":{"gain":13}}}"#;
    assert_eq!(encode_message(&packet).unwrap(), valid);
    assert_eq!(packet.validate_at(4), Err(Failure::Early));
    assert_eq!(packet.validate_at(5), Ok(()));
    assert_eq!(Transmit { sequence: 5, gain: 0 }.validate_at(4), Err(Range));
    assert_eq!(raw_digest(b"abc"), [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
        0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
        0xf2, 0x00, 0x15, 0xad,
    ]);
    assert_eq!(transmit_digest(13), Ok(raw_digest(br#"{"dir":"send","payload":{"gain":13}}"#)));
    assert!(decode_message(valid).ok() == Some(packet));
    assert!(matches!(decode_message(&[b'?'; 121]), Err(Oversize)));
    assert!(matches!(decode_message(b" {}"), Err(Malformed("json-wire"))));
    assert!(matches!(decode_message(&[valid.as_slice(), b" "].concat()), Err(Malformed("json-wire"))));
    for bad in [
        br#"{"protocol":"radio","version":8,"sequence":5,"message":{"dir":"send","payload":{"gain":0}}}"#.as_slice(),
        br#"{"protocol":"radio","version":7,"sequence":5,"message":{"dir":"send","payload":{"gain":0,"gain":1}}}"#,
        br#"{"protocol":"radio","version":7,"sequence":5,"message":{"dir":"send","payload":{"gain":0,"other":1}}}"#,
        br#"{"protocol":"radio","version":7,"sequence":5,"message":{"dir":"send","payload":[0]}}"#,
        br#"{"version":7,"protocol":"radio","sequence":5,"message":{"dir":"send","payload":{"gain":0}}}"#,
    ] {
        assert!(matches!(decode_message(bad), Err(Malformed("json-wire"))), "{bad:?}");
    }
    let range = br#"{"protocol":"radio","version":7,"sequence":5,"message":{"dir":"send","payload":{"gain":0}}}"#;
    assert!(matches!(decode_message(range), Err(Range)));
    assert!(matches!(encode_message(&Transmit { sequence: u64::MAX, gain: 0 }), Err(Range)));

    let reply = Readback { sequence: 9, reading: true };
    let bytes = encode_reply(&reply).unwrap();
    assert_eq!(bytes, br#"{"protocol":"radio","version":7,"sequence":9,"message":{"dir":"read","payload":true}}"#);
    assert!(decode_reply(&bytes).ok() == Some(reply));
    assert!(matches!(decode_message(&bytes), Err(Malformed("json-wire"))));
    assert!(matches!(decode_reply(range), Err(Malformed("json-wire"))));
}
