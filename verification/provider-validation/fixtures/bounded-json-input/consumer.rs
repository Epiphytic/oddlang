
#[test]
fn bounded_cases() {
    use std::io::BufReader;
    use crate::Fault;
    assert_eq!(item(br#"{"x":1,"x":2}"#).unwrap()["x"], 2);
    assert_eq!(item(&[b' '; LIMIT + 1]), Err(Fault::Capacity));
    assert_eq!(rows(&b"{}"[..]), Err(Fault::Input));
    assert_eq!(rows(&b"{}\n{}\n"[..]).unwrap().len(), 2);
    assert_eq!(rows(&b"{}\n{}\n{\n"[..]), Err(Fault::Capacity));
    let oversized = [b'x'; LIMIT + 1];
    assert_eq!(rows(BufReader::with_capacity(3, &oversized[..])), Err(Fault::Capacity));
}
