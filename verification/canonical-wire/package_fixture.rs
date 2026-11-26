use radio_frame_consumer::wire::{decode_upload, encode_upload, Upload, FORMAT};
use radio_frame_consumer::Failure;

fn main() {
    let data = Upload {
        sequence: 3,
        count: 7,
        bytes: vec![0, 2],
        label: "hail".into(),
        samples: vec![vec!["hail".into()]],
    };
    assert_eq!(FORMAT, 2);
    let exact = br#"{"format":2,"seq":3,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail","samples":[["hail"]]}}}"#;
    assert_eq!(encode_upload(&data).unwrap(), exact);
    assert!(decode_upload(exact).ok() == Some(data.clone()));
    assert!(matches!(decode_upload(&[b' '; 137]), Err(Failure::Limit)));
    assert_eq!(
        encode_upload(&Upload {
            bytes: vec![0; 5],
            ..data.clone()
        }),
        Err(Failure::Invalid)
    );
    assert_eq!(
        encode_upload(&Upload {
            label: "x".repeat(130),
            ..data.clone()
        }),
        Err(Failure::Limit)
    );
    assert_eq!(
        encode_upload(&Upload {
            label: "\"".repeat(40),
            ..data
        }),
        Err(Failure::Limit)
    );
    let exact_limit = Upload {
        bytes: vec![0; 4],
        label: "x".repeat(20),
        samples: vec![vec!["hail".into()]],
        sequence: 3,
        count: 7,
    };
    let wire = encode_upload(&exact_limit).unwrap();
    assert_eq!(wire.len(), 136);
    assert!(decode_upload(&wire).ok() == Some(exact_limit));
    let mut guarded = Upload {
        bytes: vec![0; 2],
        label: String::new(),
        samples: vec![vec![], vec!["hail".into(), "rain:\"\\é\x08".into()]],
        sequence: 3,
        count: 7,
    };
    // The second sample is 15 JSON bytes including its quotes and escapes.
    let wire = encode_upload(&guarded).unwrap();
    assert!(decode_upload(&wire).ok() == Some(guarded.clone()));
    guarded.samples[1][1].push('x');
    assert!(encode_upload(&guarded).is_ok());
    guarded.samples[1][1].push('x');
    assert_eq!(encode_upload(&guarded), Err(Failure::Limit));
    guarded.samples[1] = vec!["rain:\0".into()];
    assert_eq!(encode_upload(&guarded), Err(Failure::Invalid));
    guarded.samples[1] = vec!["hailstorm".into(), "rain:\u{1}\u{1}".into()];
    assert_eq!(encode_upload(&guarded), Err(Failure::Invalid));
    guarded.samples[1].reverse();
    assert_eq!(encode_upload(&guarded), Err(Failure::Limit));
    guarded.samples.clear();
    assert!(encode_upload(&guarded).is_ok());
    guarded.bytes.clear();
    assert_eq!(encode_upload(&guarded), Err(Failure::Invalid));
    let invalid_input = br#"{"format":2,"seq":3,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail","samples":[[],["hailstorm"]]}}}"#;
    assert!(matches!(
        decode_upload(invalid_input),
        Err(Failure::Invalid)
    ));
    let noncanonical = br#"{"seq":3,"format":2,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail","samples":[["hail"]]}}}"#;
    assert!(matches!(decode_upload(noncanonical), Err(Failure::Invalid)));
}
