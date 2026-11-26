mod maximum {
    include!(env!("STATIC"));
}

mod text {
    include!(env!("TEXT"));
}

fn main() {
    let n: maximum::Maximum = maximum::largest(b"18446744073709551615\n").unwrap();
    assert_eq!(n.large, u64::MAX);
    assert!(maximum::largest(b"18446744073709551616\n").is_none());
    let parsed: text::Option<'_> = text::lookup(b"a9 an_unconstrained_text_token\n").unwrap();
    assert_eq!((parsed.hex, parsed.marker), ("a9", "an_unconstrained_text_token"));
    assert!(text::lookup(b"A9 a\n").is_none());
}
