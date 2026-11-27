
use compass::Compass;
fn need<T: Copy + Clone + Eq + PartialEq>(value: T) -> T { value.clone() }
fn ord(value: Compass) -> (&'static str, isize) {
    (match value {
        Compass::Inland => "inland",
        Compass::Coast => "coast",
        Compass::Ridge => "ridge",
    }, need(value) as isize)
}
fn main() {
    for value in [Compass::Inland, Compass::Coast, Compass::Ridge] {
        println!("{:?}", ord(value));
    }
}
