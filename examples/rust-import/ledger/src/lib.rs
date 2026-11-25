pub struct Account {
    id: String,
    owner: String,
    balance: i64,
}

impl Account {
    pub fn new(id: String, owner: String, balance: i64) -> Option<Self> {
        if balance < 0 {
            return None;
        }
        Some(Self { id, owner, balance })
    }
}

pub enum TransferError {
    SourceMissing,
    DestinationMissing,
    SameAccount,
    NonPositiveAmount,
    InsufficientFunds,
    DestinationOverflow,
}

pub fn transfer(
    state: &mut std::collections::BTreeMap<String, Account>,
    source: &str,
    destination: &str,
    amount: i64,
) -> Result<(), TransferError> {
    if !state.contains_key(source) {
        return Err(TransferError::SourceMissing);
    }
    if !state.contains_key(destination) {
        return Err(TransferError::DestinationMissing);
    }
    if source == destination {
        return Err(TransferError::SameAccount);
    }
    if amount <= 0 {
        return Err(TransferError::NonPositiveAmount);
    }
    let debit_before = i128::from(state[source].balance);
    let credit_before = i128::from(state[destination].balance);
    let amount_wide = i128::from(amount);
    if debit_before < amount_wide {
        return Err(TransferError::InsufficientFunds);
    }
    let credit_after = credit_before + amount_wide;
    if credit_after > i128::from(i64::MAX) {
        return Err(TransferError::DestinationOverflow);
    }
    let debit_after = debit_before - amount_wide;
    state.get_mut(source).unwrap().balance = debit_after as i64;
    state.get_mut(destination).unwrap().balance = credit_after as i64;
    Ok(())
}
