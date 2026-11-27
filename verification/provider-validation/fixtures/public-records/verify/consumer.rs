
use indicator_core::{sampling::Reading, exports::Archived};
fn map_to_archive(item: Reading) -> Archived { item }
fn map_to_reading(item: Archived) -> Reading { item }
fn main() {
    let mut baseline: Archived = map_to_archive(Reading {
        label: "East\nDrain".into(), epoch: u64::MAX, flags: vec![0,255], active: false,
    });
    let independent: Reading = map_to_reading(baseline.clone());
    baseline.flags[0] = 1;
    let _: &String = &independent.label;
    let _: u64 = independent.epoch;
    let _: &Vec<u8> = &independent.flags;
    let _: bool = independent.active;
    assert_eq!(independent.flags, vec![0,255]);
    println!("{}", serde_json::to_string(&independent).unwrap());
}
