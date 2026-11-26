pub mod flat;
pub mod flat_limit;
mod generated;
pub mod shape_only;
pub mod singleton;

use generated::{
    decode_message, decode_reply, encode_message, encode_reply, raw_digest, transmit_digest,
    Readback, Transmit, FORMAT, PROTOCOL,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Oversize,
    Malformed(&'static str),
    Range,
    Early,
    Forbidden,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    mode: u8,
}
impl Probe {
    fn permit(&self) -> Result<(), Failure> {
        if self.mode == 1 {
            Err(Failure::Forbidden)
        } else {
            Ok(())
        }
    }
    fn estimate(&self) -> Result<u64, Failure> {
        if self.mode == 2 {
            Err(Failure::Forbidden)
        } else {
            Ok(u64::from(self.mode))
        }
    }
}
#[derive(serde::Deserialize, serde::Serialize)]
struct TokenWrapper(#[serde(with = "generated::token_hex")] generated::Token);

fn main() {
    use Failure::{Malformed, Oversize, Range};
    assert_eq!((PROTOCOL, FORMAT), ("radio", 7));
    let packet = Transmit {
        sequence: 5,
        gain: 13,
    };
    let valid = br#"{"protocol":"radio","version":7,"sequence":5,"message":{"dir":"send","payload":{"gain":13}}}"#;
    assert_eq!(encode_message(&packet).unwrap(), valid);
    let flat_packet = flat::Transmit { sequence: 5, gain: 13 };
    let flat_bytes = br#"{"dir":"send","payload":{"gain":13},"protocol":"radio","sequence":5,"version":7}"#;
    assert_eq!(flat::encode_message(&flat_packet).unwrap(), flat_bytes);
    let mut boundary = flat_limit::Transmit { sequence: 5, gain: 13 };
    assert_eq!(flat_limit::encode_message(&boundary).unwrap(), flat_bytes);
    boundary.sequence = 10;
    assert_eq!(flat_limit::encode_message(&boundary), Err(Oversize));
    assert!(matches!(flat_limit::decode_message(&[flat_bytes.as_slice(), b" "].concat()), Err(Oversize)));
    assert!(flat::decode_message(br#"{"dir":"send","message":{},"payload":{"gain":13},"protocol":"radio","sequence":5,"version":7}"#).is_err());
    let nested = flat::Scan { sequence: 7, plan: flat::Sweep::Calibration {} };
    let nested_bytes = br#"{"dir":"scan","payload":{"data":{},"kind":"cal"},"protocol":"radio","sequence":7,"version":7}"#;
    assert_eq!(flat::encode_scan(&nested).unwrap(), nested_bytes);
    assert_eq!(flat::cal_digest(), Ok(raw_digest(br#"{"kind":"cal","data":{}}"#)));
    assert!(flat::decode_message(flat_bytes).ok() == Some(flat_packet));
    assert!(flat::decode_message(valid).is_err());
    assert!(flat::decode_message(br#"{"dir":"send","payload":{"gain":13},"protocol":"radio","version":7,"sequence":5}"#).is_err());
    assert!(flat::decode_message(br#"{"dir":"send","dir":"send","payload":{"gain":13},"protocol":"radio","sequence":5,"version":7}"#).is_err());
    assert_eq!(packet.validate_at(4), Err(Failure::Early));
    assert_eq!(packet.validate_at(5), Ok(()));
    assert_eq!(
        Transmit {
            sequence: 5,
            gain: 0
        }
        .validate_at(4),
        Err(Range)
    );
    assert_eq!(
        raw_digest(b"abc"),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]
    );
    assert_eq!(
        transmit_digest(13),
        Ok(raw_digest(br#"{"dir":"send","payload":{"gain":13}}"#))
    );
    assert!(decode_message(valid).ok() == Some(packet));
    let single = singleton::Transmit {
        sequence: 5,
        gain: 13,
    };
    assert_eq!(singleton::encode_message(&single).unwrap(), valid);
    assert!(singleton::decode_message(valid).ok() == Some(single));
    assert!(matches!(decode_message(&[b'?'; 241]), Err(Oversize)));
    assert!(matches!(
        decode_message(b" {}"),
        Err(Malformed("json-wire"))
    ));
    assert!(matches!(
        decode_message(&[valid.as_slice(), b" "].concat()),
        Err(Malformed("json-wire"))
    ));
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
    assert!(matches!(
        encode_message(&Transmit {
            sequence: u64::MAX,
            gain: 0
        }),
        Err(Range)
    ));

    let reply = Readback {
        sequence: 9,
        reading: true,
    };
    let bytes = encode_reply(&reply).unwrap();
    assert_eq!(
        bytes,
        br#"{"protocol":"radio","version":7,"sequence":9,"message":{"dir":"read","payload":true}}"#
    );
    assert!(decode_reply(&bytes).ok() == Some(reply));
    assert!(matches!(
        decode_message(&bytes),
        Err(Malformed("json-wire"))
    ));
    assert!(matches!(decode_reply(range), Err(Malformed("json-wire"))));
    assert_eq!(
        Readback {
            sequence: 9,
            reading: true
        }
        .validate_at(10),
        Ok(())
    );
    assert_eq!(
        Readback {
            sequence: 9,
            reading: true
        }
        .validate_at(11),
        Err(Failure::Early)
    );
    use generated::{decode_scan, encode_scan, Scan, Sweep};
    let sample = |samples: Vec<Vec<u8>>, mode: Option<u8>, first: u8| {
        let mut token = [42; 16];
        token[0] = first;
        Scan {
            sequence: 7,
            plan: Sweep::Burst {
                samples,
                sensor: mode.map(|mode| Probe { mode }),
                token,
            },
        }
    };
    for (request, expected) in [
        (sample(vec![vec![9], vec![9]], Some(1), 0), Err(Range)),
        (sample(vec![vec![9, 7], vec![8]], Some(1), 0), Err(Oversize)),
        (sample(vec![vec![9]], Some(1), 9), Err(Range)),
        (sample(vec![vec![9]], Some(1), 8), Err(Failure::Forbidden)),
        (sample(vec![vec![9]], Some(4), 8), Err(Range)),
    ] {
        assert_eq!(encode_scan(&request), expected);
    }
    for request in [
        sample(vec![vec![9]], None, 8),
        sample(vec![vec![9]], Some(2), 8),
    ] {
        assert!(decode_scan(&encode_scan(&request).unwrap()).ok() == Some(request));
    }
    let token = [0; 16];
    assert_eq!(
        generated::cal_digest(),
        Ok(raw_digest(br#"{"kind":"cal","data":{}}"#))
    );
    assert_eq!(
        generated::burst_digest(&[vec![9, 7], vec![8]], None, token),
        Err(Oversize)
    );
    assert_eq!(generated::burst_digest(&[vec![9]], None, token),
        Ok(raw_digest(br#"{"kind":"burst","data":{"samples":["CQ=="],"sensor":null,"token":"00000000000000000000000000000000"}}"#)));
    let exported = serde_json::to_string(&TokenWrapper([7; 16])).unwrap();
    assert_eq!(exported, format!("\"{}\"", "07".repeat(16)));
    assert_eq!(
        serde_json::from_str::<TokenWrapper>(&exported).unwrap().0,
        [7; 16]
    );
    let token = [0; 16];
    for samples in [vec![vec![3]; 5], vec![vec![9; 4]], vec![vec![3; 3]; 3]] {
        let invalid = shape_only::Scan {
            sequence: 0,
            plan: shape_only::Sweep::Burst {
                samples: samples.clone(),
                sensor: None,
                token,
            },
        };
        assert_eq!(
            shape_only::encode_scan(&invalid),
            Err(Malformed("json-wire"))
        );
        assert_eq!(
            shape_only::burst_digest(&samples, None, token),
            Err(Malformed("json-wire"))
        );
    }
    for samples in [vec![vec![1; 3]; 2], vec![vec![]; 4]] {
        let input = shape_only::Scan {
            sequence: 0,
            plan: shape_only::Sweep::Burst {
                samples: samples.clone(),
                sensor: None,
                token,
            },
        };
        let bytes = shape_only::encode_scan(&input).unwrap();
        assert!(shape_only::decode_scan(&bytes).ok() == Some(input));
        assert!(shape_only::burst_digest(&samples, None, token).is_ok());
    }
}
