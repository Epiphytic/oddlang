
use test_sample::Bearing;
fn main() {
    let first = Bearing::East;
    let second = Bearing::West;
    assert_eq!(first, first.clone());
    assert_ne!(first, second);
    println!("{first:?}={} {second:?}={}", first.clone() as isize, second.clone() as isize);
}
