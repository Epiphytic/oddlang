enum Band { Low = 3, High = 9 }

impl Copy for Band {}
impl Clone for Band { fn clone(&self) -> Self { *self } }

struct Reading {
    sequence: u64,
    station: [u8; 2],
    band: Band,
    caption: Vec<u8>,
    powered: bool,
}
fn counter(target: &mut Vec<u8>, value: u64) { target.extend(value.to_be_bytes()); }
fn length(target: &mut Vec<u8>, value: u32) { target.extend(value.to_be_bytes()); }
fn packet(target: &mut Vec<u8>, data: &[u8]) {
    length(target, data.len() as u32);
    target.extend(data);
}
fn encode(sink: &mut Vec<u8>, row: &Reading) {
    counter(sink, row.sequence);
    sink.extend(row.station);
    sink.push(row.band as u8);
    packet(sink, &row.caption);
    sink.extend([u8::from(row.powered)]);
}
fn main() {
    for band in [Band::Low, Band::High] {
        let mut bytes = Vec::new();
        encode(&mut bytes, &Reading { sequence: 257, station: [9, 0], band,
            caption: vec![0, 255, 65], powered: true });
        for byte in bytes { print!("{byte:02x}"); }
        println!();
    }
}
