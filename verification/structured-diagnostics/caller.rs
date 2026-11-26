fn main() {
    use station_observer::records::{begin, end, mark, observation};
    let mut current = "initial";
    mark(&mut current, "before");
    assert_eq!(current, "before");
    std::thread::spawn(begin).join().unwrap();
    begin();
    observation("númeró", "control\u{0001}\n");
    mark(&mut current, "after");
    end();
    observation("suppressed", "not emitted");
    assert_eq!(current, "after");
    println!("{current}");
}
