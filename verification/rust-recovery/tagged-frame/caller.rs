
fn main() {
    for packet in [Packet::Key { first: [0x41, 0x91], second: [0x33, 0x66] },
                   Packet::Reading { value: 0x0102030405060708, footer: [0x0f, 0x7e] }] {
        for byte in packet.encode().unwrap() { print!("{byte:02x}"); }
        println!();
    }
}
