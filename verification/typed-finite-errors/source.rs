use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentReason { Missing, Omitted }
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum ArchiveFault {
    #[error("archive unavailable")]
    Unavailable,
    #[error("document rejected: {0:?}")]
    Rejected(DocumentReason),
    #[error("reader full")]
    Full { first_byte: u64, capacity: u32 }
}
pub type Outcome<T> = std::result::Result<T, ArchiveFault>;
