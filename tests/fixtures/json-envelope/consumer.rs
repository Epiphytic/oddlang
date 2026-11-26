#[cfg(test)]
mod baseline;
#[cfg(test)]
mod renamed;
#[cfg(test)]
mod reordered;
#[cfg(test)]
mod tightened;
#[cfg(test)]
mod tuple;

#[test]
fn caller_keeps_the_bound_api_and_wire_contract() {
    use baseline::{Bounds, ReadError, Thumbnail};
    let _ = Bounds::default();
    assert_eq!(ReadError::TooLarge as isize, 0);
    assert_eq!(ReadError::Malformed as isize, 1);
    assert_eq!(ReadError::UnexpectedRevision as isize, 2);
    assert_eq!(ReadError::BadBinary as isize, 3);
    assert_eq!(
        (Bounds::PIXELS, Bounds::MESSAGE, Bounds::RECORD),
        (4, 52, 80)
    );
    assert_eq!(
        Thumbnail::write(&[0, 255]).unwrap(),
        br#"{"rev":13,"pixels":"AP8="}"#
    );
    assert_eq!(Thumbnail::write(&[]).unwrap(), br#"{"rev":13,"pixels":""}"#);
    for bytes in [&[][..], &[0, 255], &[1, 2, 3, 4]] {
        assert_eq!(
            Thumbnail::read(&Thumbnail::write(bytes).unwrap()).unwrap(),
            bytes
        );
    }
    assert_eq!(
        Thumbnail::read(br#" { "pixels" : "AP8=", "rev":13 } "#).unwrap(),
        [0, 255]
    );
    assert_eq!(
        Thumbnail::read(br#"{"rev":13,"pixels":"\u0041P8="}"#).unwrap(),
        [0, 255]
    );
    let mut max_wire = br#"{"rev":13,"pixels":""}"#.to_vec();
    max_wire.resize(52, b' ');
    assert!(Thumbnail::read(&max_wire).unwrap().is_empty());
    max_wire.push(b' ');
    assert_eq!(Thumbnail::read(&max_wire), Err(ReadError::TooLarge));
    assert_eq!(
        ReadError::BadBinary.to_string(),
        "invalid thumbnail pixel encoding"
    );
}

#[test]
fn strict_shape_and_base64_and_guard_precedence() {
    use baseline::{ReadError::*, Thumbnail};
    for bytes in [
        &br#"{"rev":13,"pixels":"","extra":0}"#[..],
        br#"{"rev":13,"rev":13,"pixels":""}"#,
        br#"{"rev":13,"pixels":"","pixels":""}"#,
        br#"{"rev":13}"#,
        br#"{"pixels":""}"#,
        br#"{"rev":-1,"pixels":""}"#,
        br#"{"rev":4294967296,"pixels":""}"#,
        br#"{"rev":13.0,"pixels":""}"#,
        br#"{"rev":13,"pixels":null}"#,
        br#"{"rev":13,"pixels":""} trailing"#,
        br#"{"rev":0,"pixels":"","extra":1}"#,
        br#"[13,""]"#,
    ] {
        assert_eq!(Thumbnail::read(bytes), Err(Malformed), "{bytes:?}");
    }
    for bytes in [
        &br#"{"rev":13,"pixels":"/x=="}"#[..],
        br#"{"rev":13,"pixels":"/w"}"#,
        br#"{"rev":13,"pixels":"_w=="}"#,
        br#"{"rev":13,"pixels":" /w=="}"#,
    ] {
        assert_eq!(Thumbnail::read(bytes), Err(BadBinary), "{bytes:?}");
    }
    assert_eq!(
        Thumbnail::read(br#"{"rev":0,"pixels":"!!!!!!!!!!!!"}"#),
        Err(UnexpectedRevision)
    );
    assert_eq!(
        Thumbnail::read(br#"{"rev":13,"pixels":"!!!!!!!!!!!!"}"#),
        Err(TooLarge)
    );
    assert_eq!(
        Thumbnail::read(br#"{"rev":13,"pixels":"AQIDBAU="}"#),
        Err(TooLarge)
    );
    assert_eq!(Thumbnail::read(&[b'!'; 53]), Err(TooLarge));
    assert_eq!(Thumbnail::write(&[0; 5]), Err(TooLarge));
}

#[test]
fn graph_mutations_change_wire_bytes_and_guard_outcomes() {
    assert_eq!(
        renamed::Thumbnail::write(&[255]).unwrap(),
        br#"{"octets":"/w==","revision":13}"#
    );
    assert_eq!(
        renamed::Thumbnail::read(br#"{"rev":13,"pixels":"/w=="}"#),
        Err(renamed::ReadError::Malformed)
    );
    assert_eq!(tightened::Bounds::PIXELS, 1);
    assert_eq!(tightened::Bounds::MESSAGE, 22);
    assert_eq!(
        tightened::Thumbnail::write(&[0]),
        Err(tightened::ReadError::TooLarge)
    );
    assert_eq!(
        tightened::Thumbnail::write(&[0; 2]),
        Err(tightened::ReadError::TooLarge)
    );
    assert!(
        tightened::Thumbnail::read(br#"{"rev":13,"pixels":""}"#)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        reordered::Thumbnail::read(br#"{"rev":0,"pixels":"!!!!!!!!!!!!"}"#),
        Err(reordered::ReadError::TooLarge)
    );
}

#[test]
fn tuple_policy_uses_an_exact_positional_shape_on_decode() {
    use tuple::{ReadError, Thumbnail};
    assert_eq!(Thumbnail::read(br#"[13,"/w=="]"#).unwrap(), [255]);
    assert_eq!(
        Thumbnail::write(&[255]).unwrap(),
        br#"{"rev":13,"pixels":"/w=="}"#
    );
    assert_eq!(
        Thumbnail::read(br#"[0,""]"#),
        Err(ReadError::UnexpectedRevision)
    );
    for bytes in [&br#"[13,"",0]"#[..], br#"[13]"#, br#"["",13]"#] {
        assert_eq!(Thumbnail::read(bytes), Err(ReadError::Malformed));
    }
}
