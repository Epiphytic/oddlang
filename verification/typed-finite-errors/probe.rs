include!("projection/errors.rs");

fn discriminant(reason: DocumentReason) -> u8 {
    match reason {
        DocumentReason::Missing => 0,
        DocumentReason::Omitted => 1,
    }
}

fn main() {
    let reason = DocumentReason::Omitted;
    assert_eq!(reason.clone(), reason);
    assert_eq!(reason as u8, discriminant(reason));
    assert_eq!(format!("{reason:?}"), "Omitted");
    let absent: Outcome<()> = Err(ArchiveFault::Rejected(reason));
    let rejected = absent.unwrap_err();
    assert_eq!(rejected, ArchiveFault::Rejected(reason));
    assert_eq!(rejected.to_string(), "document rejected: Omitted");
    assert_eq!(format!("{rejected:?}"), "Rejected(Omitted)");
    let full = ArchiveFault::Full { first_byte: u64::MAX, capacity: u32::MAX };
    assert!(::std::error::Error::source(&full).is_none());
    match full {
        ArchiveFault::Unavailable | ArchiveFault::Rejected(_) => panic!("wrong value"),
        ArchiveFault::Full { first_byte, capacity } => {
            assert_eq!(first_byte, u64::MAX);
            assert_eq!(capacity, u32::MAX);
        }
    }
    println!("{}", ArchiveFault::Unavailable);
}
