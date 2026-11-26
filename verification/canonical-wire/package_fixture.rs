use radio_frame_consumer::wire::{decode_upload, encode_upload, Sample, Upload, FORMAT};
use radio_frame_consumer::Failure;

fn sample(tag: &str) -> Sample {
    Sample { tag: tag.into() }
}

fn main() {
    let data = Upload {
        sequence: 3,
        count: 7,
        bytes: vec![0, 2],
        label: "hail".into(),
        samples: vec![vec![sample("hail")]],
    };
    assert_eq!(FORMAT, 2);
    let exact = br#"{"format":2,"seq":3,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail","samples":[[{"tag":"hail"}]]}}}"#;
    assert_eq!(encode_upload(&data).unwrap(), exact);
    assert!(decode_upload(exact).ok() == Some(data.clone()));
    assert!(matches!(decode_upload(&[b' '; 161]), Err(Failure::Limit)));
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
        label: "x".repeat(36),
        samples: vec![vec![sample("hail")]],
        sequence: 3,
        count: 7,
    };
    let wire = encode_upload(&exact_limit).unwrap();
    assert_eq!(wire.len(), 160);
    assert!(decode_upload(&wire).ok() == Some(exact_limit));
    let mut guarded = Upload {
        bytes: vec![0; 2],
        label: String::new(),
        samples: vec![vec![], vec![sample("hail"), sample("rain:\"\\é\x08")]],
        sequence: 3,
        count: 7,
    };
    // The second sample is 15 JSON bytes including its quotes and escapes.
    let wire = encode_upload(&guarded).unwrap();
    assert!(decode_upload(&wire).ok() == Some(guarded.clone()));
    guarded.samples[1][1].tag.push('x');
    assert!(encode_upload(&guarded).is_ok());
    guarded.samples[1][1].tag.push('x');
    assert_eq!(encode_upload(&guarded), Err(Failure::Limit));
    guarded.samples[1] = vec![sample("rain:\0")];
    assert_eq!(encode_upload(&guarded), Err(Failure::Invalid));
    guarded.samples[1] = vec![sample("hailstorm"), sample("rain:\u{1}\u{1}")];
    assert_eq!(encode_upload(&guarded), Err(Failure::Invalid));
    guarded.samples[1].reverse();
    assert_eq!(encode_upload(&guarded), Err(Failure::Limit));
    guarded.samples.clear();
    assert!(encode_upload(&guarded).is_ok());
    guarded.bytes.clear();
    assert_eq!(encode_upload(&guarded), Err(Failure::Invalid));
    let invalid_input = br#"{"format":2,"seq":3,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail","samples":[[],[{"tag":"hailstorm"}]]}}}"#;
    assert!(matches!(
        decode_upload(invalid_input),
        Err(Failure::Invalid)
    ));
    let noncanonical = br#"{"seq":3,"format":2,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail","samples":[[{"tag":"hail"}]]}}}"#;
    assert!(matches!(decode_upload(noncanonical), Err(Failure::Invalid)));
}
