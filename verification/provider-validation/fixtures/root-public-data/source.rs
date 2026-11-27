
#![cfg(unix)]
use thiserror::Error;
#[derive(Debug, Error)]
pub enum Fault {
    #[error("sample missing")]
    Empty,
    #[error("sensor unavailable")]
    Unavailable,
}
#[derive(Debug)]
pub struct Snapshot {
    pub secondary: String,
    pub primary: String,
}
pub fn is_empty(_: &Snapshot) -> bool { false }
