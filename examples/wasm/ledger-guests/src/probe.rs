// Independent ABI probe, retaining the host's original response verbatim.
use super::{Account, Guest, TransferError, initial_state};

thread_local! {
    static RESPONSE: Vec<Account> = initial_state();
}

pub struct Probe;

impl Guest for Probe {
    fn lookup(id: String) -> Option<Account> {
        RESPONSE.with(|rows| rows.iter().find(|row| row.id == id).cloned())
    }

    fn transfer(_: String, _: String, _: i64) -> Result<(), TransferError> {
        RESPONSE.with(|_| Err(TransferError::SourceMissing))
    }
}
