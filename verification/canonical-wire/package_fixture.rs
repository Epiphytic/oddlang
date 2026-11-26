use radio_frame_consumer::wire::{decode_upload, encode_upload, Upload, FORMAT};
use radio_frame_consumer::Failure;

fn main() {
    let data = Upload {
        sequence: 3,
        count: 7,
        bytes: vec![0, 2],
        label: "hail".into(),
    };
    assert_eq!(FORMAT, 2);
    let exact = br#"{"format":2,"seq":3,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail"}}}"#;
    assert_eq!(encode_upload(&data).unwrap(), exact);
    assert!(decode_upload(exact).ok() == Some(data.clone()));
    assert!(matches!(decode_upload(&[b' '; 116]), Err(Failure::Limit)));
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
    let noncanonical = br#"{"seq":3,"format":2,"packet":{"tag":"upload","data":{"count":7,"bytes":"AAI=","label":"hail"}}}"#;
    assert!(matches!(decode_upload(noncanonical), Err(Failure::Invalid)));
}
