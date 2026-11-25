use super::{Account, Guest, TransferError, initial_state};
#[cfg(feature = "generated")]
use crate::generated::State;
#[cfg(not(feature = "generated"))]
use crate::manual::State;
use std::cell::RefCell;

thread_local! {
    static STATE: RefCell<State> = RefCell::new({
        let accounts = initial_state();
        #[cfg(feature = "double-import")]
        let _ = initial_state();
        State::from_accounts(accounts)
    });
}

pub struct Ledger;

impl Guest for Ledger {
    fn lookup(id: String) -> Option<Account> {
        STATE.with_borrow(|state| state.lookup(&id))
    }

    fn transfer(source: String, destination: String, amount: i64) -> Result<(), TransferError> {
        let result = STATE.with_borrow_mut(|state| state.transfer(&source, &destination, amount));
        #[cfg(feature = "trap")]
        if result.is_err() {
            core::arch::wasm32::unreachable();
        }
        result
    }

    #[cfg(feature = "instrumented")]
    fn fixture_load(accounts: Vec<Account>) {
        STATE.with_borrow_mut(|state| *state = State::from_accounts(accounts));
    }

    #[cfg(feature = "instrumented")]
    fn complete_snapshot() -> Vec<Account> {
        STATE.with_borrow(State::snapshot)
    }
}
