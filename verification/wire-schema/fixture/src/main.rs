#[path = "../input.rs"]
mod original;
mod projected;
#[allow(dead_code)]
mod single;
#[allow(dead_code)]
mod strict;

use projected::{Irrigation, Packet, Sensor};

fn main() {
    let missing = r#"{"channel":3}"#;
    assert!(serde_json::from_str::<strict::Sensor>(missing).is_err());
    assert!(serde_json::from_str::<Sensor>(missing).unwrap().meter.is_none());
    for value in ["null", "0", "9007199254740993", "18446744073709551615"] {
        let wire = format!(r#"{{"channel":3,"meter":{value}}}"#);
        let sensor: strict::Sensor = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&sensor).unwrap(), wire);
    }
    assert_eq!(
        serde_json::to_string(&Irrigation::Sample(Sensor {
            channel: 3,
            meter: None
        }))
        .unwrap(),
        r#"{"event":"sample","value":{"channel":3,"meter":null}}"#,
    );
    assert_eq!(
        serde_json::to_string(&Irrigation::Idle).unwrap(),
        r#"{"event":"idle"}"#
    );
    let adjust = Irrigation::AdjustRate {
        enabled: true,
        flow: 12,
        tags: vec![1, 2],
    };
    let reference = original::Irrigation::AdjustRate {
        enabled: true,
        flow: 12,
        tags: vec![1, 2],
    };
    assert_eq!(
        serde_json::to_vec(&adjust).unwrap(),
        serde_json::to_vec(&reference).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&adjust).unwrap(),
        r#"{"event":"adjust_rate","value":{"enabled":true,"flow":12,"tags":[1,2]}}"#
    );
    for bad in [
        r#"{"event":"sample","value":{"channel":3,"meter":null,"extra":true}}"#,
        r#"{"event":"sample","value":{"channel":3,"channel":4}}"#,
        r#"{"event":"sample","event":"idle","value":{"channel":3}}"#,
        r#"{"event":"unknown"}"#,
    ] {
        assert!(serde_json::from_str::<Irrigation>(bad).is_err(), "{bad}");
    }
    assert_eq!(adjust.validate_wire(), Ok(()));
    let singleton = single::Irrigation::AdjustRate {
        enabled: true,
        flow: 0,
        tags: vec![7],
    };
    assert_eq!(singleton.validate_wire(), Err("urn:garden:flow"));
    let enabled = Irrigation::AdjustRate {
        enabled: true,
        flow: 0,
        tags: vec![1, 1],
    };
    let disabled = Irrigation::AdjustRate {
        enabled: false,
        flow: 0,
        tags: vec![1, 1],
    };
    assert_eq!(enabled.validate_wire(), Err("urn:garden:flow"));
    assert_eq!(disabled.validate_wire(), Err("urn:garden:tag"));
    let bytes = r#"{"signature":"00ab","chunks":["AQID","/w=="]}"#;
    let packet = serde_json::from_str::<Packet>(bytes).unwrap();
    let reference = serde_json::from_str::<original::Packet>(bytes).unwrap();
    assert_eq!(packet.chunks, vec![vec![1, 2, 3], vec![255]]);
    assert_eq!(
        serde_json::to_string(&packet).unwrap(),
        serde_json::to_string(&reference).unwrap()
    );
    for (value, expected) in [
        (r#"["!","",""]"#, "piece list limit"),
        (r#"["!!!!!!!!!!!!"]"#, "piece text limit"),
        (r#"["AAAAAAA="]"#, "piece byte limit"),
        (r#"["AB=="]"#, "piece encoding"),
        (r#"["AQIDBA==","AA=="]"#, "piece list limit"),
    ] {
        let document = format!(r#"{{"signature":"00ab","chunks":{value}}}"#);
        let err = serde_json::from_str::<Packet>(&document).err().unwrap();
        let original = serde_json::from_str::<original::Packet>(&document)
            .err()
            .unwrap();
        assert!(err.to_string().starts_with(expected));
        assert!(original.to_string().starts_with(expected));
    }
    for malformed in ["00AB", "zzzz", ""] {
        let text = bytes.replace("00ab", malformed);
        let projected = serde_json::from_str::<Packet>(&text).err().unwrap();
        let source = serde_json::from_str::<original::Packet>(&text)
            .err()
            .unwrap();
        assert!(projected.to_string().starts_with("signature text"));
        assert!(source.to_string().starts_with("signature text"));
    }
}
