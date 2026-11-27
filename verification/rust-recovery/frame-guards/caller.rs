
fn main() {
    for (seq, n) in [(10, 0), (10, 5), (0, 0), (9, 1), (8, 1), (u64::MAX, 1), (5, 5), (0, 2)] {
        match (Parcel::Chunk { seq, contents: vec![0x51; n] }).encode() {
            Ok(bytes) => { for byte in bytes { print!("{byte:02x}"); } println!(); }
            Err(error) => println!("{error:?}"),
        }
    }
}
