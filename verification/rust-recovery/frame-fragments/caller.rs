
fn main() {
    let head = |version| Envelope::Head(Head { first: [0x01, 0x62], second: [0x31, 0x9b], version }, Level::High);
    let copy = |count| Envelope::Copy(Copy { first: [0x71, 0xf2], second: [0x33, 0x2b], count });
    for frame in [head(1), head(2), copy(None), copy(Some(3)), copy(Some(2))] {
        match frame.encode() {
            Ok(data) => { for byte in data { print!("{byte:02x}"); } println!(); }
            Err(fault) => println!("{fault:?}"),
        }
    }
}
