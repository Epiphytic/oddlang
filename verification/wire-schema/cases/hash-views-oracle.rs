// SHA-256 preimages in hex: 00ff010200000002faf02a (On),
// 00ff010200000002faf011 (Off), and 470102 (label).
#![deny(warnings)]
mod wire;
#[derive(serde::Serialize, serde::Deserialize)]
pub enum State {
    On,
    Off,
}
fn main() {
    let mut sample = wire::Sample {
        seq: 0x0102,
        payload: vec![0xfa, 0xf0],
    };
    assert_eq!(
        sample.checksum(State::On),
        Ok([
            164, 179, 7, 89, 174, 159, 189, 69, 66, 192, 19, 93, 2, 249, 245, 27, 70, 23, 5, 231,
            198, 107, 162, 151, 45, 160, 147, 239, 213, 78, 100, 212
        ])
    );
    assert_eq!(
        sample.checksum(State::Off),
        Ok([
            145, 168, 72, 19, 15, 66, 226, 95, 15, 249, 152, 121, 182, 46, 65, 59, 78, 89, 53, 233,
            249, 44, 82, 234, 208, 197, 171, 149, 240, 20, 142, 220
        ])
    );
    sample.payload.clear();
    assert_eq!(sample.checksum(State::On), Err("urn:sensor:bounds"));
    assert_eq!(
        sample.label(),
        "sensor-9c5f61a4d74da3b7ce70f5bf231378d6b3ca022edec62772d5e50dfad41dab32"
    );
}
