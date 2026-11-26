mod generated {
    include!(env!("GENERATED"));
}

fn main() {
    use generated::read_report;
    let first = b"stock 1\nbatch v5.2-rc.3\ntrack west=4.7\nunit probe A=7 af42 0664 15\nready yes\nend\n";
    let accepted = read_report(first).expect("unrelated inventory is accepted");
    let _: &str = accepted.batch;
    let _: &'static str = accepted.item;
    let _: [u8; 2] = accepted.hash;
    let _: u16 = accepted.limit;
    let _: u32 = accepted.amount;
    let _: bool = accepted.ready;
    assert_eq!((accepted.batch, accepted.track, accepted.family), ("v5.2-rc.3", "west=4.7", "west"));
    assert_eq!((accepted.count, accepted.item, accepted.hash), (7, "probe", [0xaf, 0x42]));
    assert_eq!((accepted.limit, accepted.amount, accepted.ready), (0o664, 15, true));
    for text in [
        "stock 2\nbatch v5.2-rc.3\ntrack west=4.7\nunit probe A=7 af42 0664 15\nready yes\nend\n",
        "stock 1\nbatch v5.02\ntrack west=4.7\nunit probe A=7 af42 0664 15\nready yes\nend\n",
        "stock 1\nbatch v5.2\ntrack west=4.0\nunit probe A=7 af42 0664 15\nready yes\nend\n",
        "stock 1\nbatch v5.2\ntrack east=4.257\nunit probe A=7 af42 0664 15\nready no\nend\n",
        "stock 1\nbatch v5.2\ntrack east=4.5\nunit probe A=7 Af42 0664 15\nready no\nend\n",
        "stock 1\nbatch v5.2\ntrack east=4.5\nunit probe A=7 af42 0664 17\nready no\nend\n",
        "stock 1\nbatch v5.2\ntrack east=4.5\nunit probe A=7 af42 0664 16\nready no\nend",
        "stock 1\nbatch v5.2\ntrack east=4.5\nunit probe A=7 af42 0664 16\nready no\nend\n\n",
        "stock 1\nbatch v5.2\ntrack east=4.5\nunit probe A=7 af42 0664 16\nready  no\nend\n",
        "stock 1\nbatch v5.2\ntrack east=4.5\nunit probe A=7 af42 0664 16\nready\tno\nend\n",
    ] {
        assert!(read_report(text.as_bytes()).is_none(), "accepted {text:?}");
    }
    assert!(read_report(&[0xff, b'\n']).is_none());
    let boundary = b"stock 1\nbatch v5.2\ntrack east=4.16\nunit probe B=7 af42 0664 16\nready no\nend\n";
    let boundary = read_report(boundary).unwrap();
    assert_eq!(boundary.amount, 16);
    assert!(!boundary.ready);
}
