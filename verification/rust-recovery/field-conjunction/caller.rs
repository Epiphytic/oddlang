
fn main() {
    let value = Invoice { serial: 3, flags: vec![17], stage: Stage::Sent };
    for (serial, backup, flags, stage) in [
        (3, 0, 17, Stage::Sent), (0, 3, 17, Stage::Sent),
        (3, 3, 18, Stage::Sent), (3, 3, 17, Stage::Draft),
    ] {
        let other = Receipt { serial, backup, flags: vec![flags], stage };
        println!("{}", value.agrees(&other));
    }
}
