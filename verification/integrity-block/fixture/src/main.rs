use codec::{pack, unpack, Packing, Tile};
use std::fmt::{Display, Formatter};

mod all_messages;
mod codec;
mod mutant;
mod narrower;
mod stored;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Capacity,
    Position,
    Damaged,
    Field(&'static str),
}

impl Display for Failure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

fn error<T>(result: Result<T, Failure>, expected: Failure) {
    assert!(matches!(result, Err(actual) if actual == expected));
}

fn main() {
    use Failure::{Capacity, Damaged, Field, Position};
    let identity = pack(254, &[0, 255], Packing::Deflated).unwrap();
    assert_eq!(identity.packing, Packing::Stored);
    assert_eq!(identity.end(), Ok(256));
    assert_eq!(unpack(&identity, Packing::Deflated).unwrap(), [0, 255]);
    error(pack(255, &[0, 255], Packing::Stored), Position);
    error(pack(u64::MAX, &[0], Packing::Stored), Position);
    error(pack(u64::MAX, &[], Packing::Stored), Capacity);
    error(pack(0, &[42; 193], Packing::Stored), Capacity);

    let baseline = serde_json::to_string(&identity).unwrap();
    assert_eq!(
        baseline,
        r#"{"position":254,"mode":"stored","width":2,"fingerprint":"06eb7d6a69ee19e5fbdf749018d3d2abfa04bcbd1365db312eb86dc7169389b8","pixels":"AP8="}"#
    );
    let value = serde_json::to_value(&identity).unwrap();
    let fields = ["position", "mode", "width", "fingerprint", "pixels"];
    let mut tuple: Vec<_> = fields.iter().map(|key| value[*key].clone()).collect();
    assert!(serde_json::from_value::<Tile>(serde_json::json!(tuple)).unwrap() == identity);
    tuple.push(serde_json::json!(false));
    assert!(serde_json::from_value::<Tile>(serde_json::json!(tuple)).is_err());
    for text in [
        baseline.replace(r#""position":254"#, r#""position":254,"position":254"#),
        baseline.replace(r#""position":254"#, r#""location":254"#),
        baseline.replace("06eb", "06EB"),
        baseline.replace("AP8=", "AP8"),
        baseline.replace("AP8=", "AP9="),
        baseline.replace("AP8=", "_P8="),
    ] {
        assert!(serde_json::from_str::<Tile>(&text).is_err());
    }
    let invalid = baseline.replace("AP8=", "AP8");
    assert!(
        matches!(serde_json::from_str::<Tile>(&invalid), Err(e) if e.to_string().starts_with("invalid pixels"))
    );
    let plain = stored::pack(18, &[4, 7], stored::Packing::Stored).unwrap();
    assert_eq!(plain.end().unwrap(), 20);
    assert_eq!(
        stored::unpack(&plain, stored::Packing::Stored).unwrap(),
        [4, 7]
    );
    error(codec::field_codec::decode_pixels("AAA=", 1), Field("width"));
    error(
        codec::field_codec::decode_pixels("A===", 1),
        Field("pixels"),
    );
    error(codec::field_codec::decode_pixels("!!!!!!!!", 1), Capacity);

    let complete = all_messages::pack(0, &[4], all_messages::Packing::Stored).unwrap();
    assert_eq!(complete.end(), Ok(1));
    assert_eq!(
        all_messages::unpack(&complete, all_messages::Packing::Stored).unwrap(),
        [4]
    );
    assert!(
        matches!(serde_json::from_str::<all_messages::Tile>(&invalid),
                     Err(error) if error.to_string().starts_with("wire payloadSyntax"))
    );

    let small = narrower::pack(63, &[4], narrower::Packing::Stored).unwrap();
    assert_eq!(small.end(), Ok(64));
    assert_eq!(
        narrower::unpack(&small, narrower::Packing::Stored).unwrap(),
        [4]
    );
    error(
        narrower::pack(0, &[42; 65], narrower::Packing::Deflated),
        Capacity,
    );

    let raw = [42; 128];
    let compressed = pack(0, &raw, Packing::Deflated).unwrap();
    assert_eq!(compressed.packing, Packing::Deflated);
    assert!(compressed.pixels.len() < raw.len());
    assert_eq!(unpack(&compressed, Packing::Deflated).unwrap(), raw);
    error(unpack(&compressed, Packing::Stored), Damaged);

    // Independently prepared using Python zlib, including one dictionary member.
    let member = [120, 156, 211, 210, 26, 88, 0, 0, 75, 75, 21, 1];
    let dictionary_member = [120, 187, 27, 34, 4, 129, 211, 26, 96, 0, 0, 75, 75, 21, 1];
    let mut frame = compressed.clone();
    frame.pixels = member.to_vec();
    assert_eq!(unpack(&frame, Packing::Deflated).unwrap(), raw);
    for bad in [
        member[..11].to_vec(),
        [member.as_slice(), &[0]].concat(),
        [member.as_slice(), member.as_slice()].concat(),
        dictionary_member.to_vec(),
    ] {
        frame.pixels = bad;
        error(unpack(&frame, Packing::Deflated), Damaged);
    }
    frame.pixels = member.to_vec();
    for width in [127, 129] {
        frame.width = width;
        error(unpack(&frame, Packing::Deflated), Damaged);
    }
    frame.width = 128;
    frame.fingerprint[0] ^= 1;
    error(unpack(&frame, Packing::Deflated), Damaged);
    frame.width = member.len() as u32;
    error(frame.check(), Damaged);

    let altered = mutant::pack(30, &[5], mutant::Packing::Stored).unwrap();
    assert_eq!(altered.end(), Ok(31));
    assert!(serde_json::to_string(&altered)
        .unwrap()
        .starts_with(r#"{"location":30,"#));
    assert_eq!(
        mutant::unpack(&altered, mutant::Packing::Stored).unwrap(),
        [5]
    );
    let oversized = mutant::Tile {
        at: u64::MAX,
        width: 0,
        ..altered
    };
    error(oversized.check(), Position);
}
