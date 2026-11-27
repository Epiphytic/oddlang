extern crate records;
use records::Counter;

fn main() {
    for active in [false, true] {
        for count in [0u32, 1, u32::MAX] {
            let item = Counter { active, count };
            assert_eq!((item.active, item.count), (active, count));
        }
    }
}
