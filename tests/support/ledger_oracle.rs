use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Account {
    pub id: String,
    pub owner: String,
    pub balance: i64,
}

pub type State = BTreeMap<String, Account>;

/// Apply the v1 transfer table to a fixture with valid entry balances.
pub fn transfer(
    state: &mut State,
    source: &str,
    destination: &str,
    amount: i64,
) -> Result<(), &'static str> {
    let debit = state
        .get(source)
        .ok_or("urn:oddlang:ledger:error:SourceMissing")?
        .balance;
    let credit = state
        .get(destination)
        .ok_or("urn:oddlang:ledger:error:DestinationMissing")?
        .balance;
    if source == destination {
        return Err("urn:oddlang:ledger:error:SameAccount");
    }
    if amount <= 0 {
        return Err("urn:oddlang:ledger:error:NonPositiveAmount");
    }
    if debit < amount {
        return Err("urn:oddlang:ledger:error:InsufficientFunds");
    }
    if amount > i64::MAX - credit {
        return Err("urn:oddlang:ledger:error:DestinationOverflow");
    }

    state.get_mut(source).expect("source exists").balance = debit - amount;
    state
        .get_mut(destination)
        .expect("destination exists")
        .balance = credit + amount;
    Ok(())
}
