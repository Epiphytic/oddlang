#[derive(Debug, PartialEq, Eq)]
enum Fault {
    Invalid(&'static str),
}

mod generated {
    include!(env!("GENERATED"));
}

fn main() {
    use generated::{LogBook, Sample};
    let original = b"log 2\nmode indoor active\nrows 7 9\nitem top 7\nitem lower 9\n";
    let report = LogBook::parse(original).unwrap();
    assert_eq!(report.mode, "indoor");
    let _: [u16; 2] = report.counts;
    assert_eq!(report.counts, [7, 9]);
    let _: Vec<Sample> = report.samples.clone();
    assert_eq!(report.samples[0].name, "top");
    assert_eq!(report.samples[1].amount, 9);
    assert_eq!(report.key(), "indoor");
    assert_eq!(report.resource("sample"), "sample:indoor");
    for invalid in [
        b"log 2\nmode indoor inactive\nrows 7 9\nitem top 7\nitem lower 9\n".as_slice(),
        b"log 2\nmode indoor active\nrows 7 9\nitem top 7\nitem lower 10\n",
        b"log 2\nmode indoor active\nrows 7 9\nitem top 1\nitem lower 18446744073709551615\n",
    ] {
        assert_eq!(LogBook::parse(invalid).unwrap_err(), Fault::Invalid("bad_log"));
    }
}
