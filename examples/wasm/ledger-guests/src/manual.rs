use super::{Account, TransferError};
use std::collections::BTreeMap;

pub struct State(BTreeMap<String, Account>);

impl State {
    pub fn from_accounts(accounts: Vec<Account>) -> Self {
        let mut rows = BTreeMap::new();
        for account in accounts {
            assert!(account.balance >= 0, "negative initial balance");
            rows.insert(account.id.clone(), account);
        }
        Self(rows)
    }

    pub fn lookup(&self, id: &str) -> Option<Account> {
        self.0.get(id).cloned()
    }

    #[cfg(feature = "instrumented")]
    pub fn snapshot(&self) -> Vec<Account> {
        self.0.values().cloned().collect()
    }

    pub fn transfer(
        &mut self,
        source: &str,
        destination: &str,
        amount: i64,
    ) -> Result<(), TransferError> {
        let payer = self.0.get(source).ok_or(TransferError::SourceMissing)?;
        let payee = self
            .0
            .get(destination)
            .ok_or(TransferError::DestinationMissing)?;
        if source == destination {
            return Err(TransferError::SameAccount);
        }
        if amount <= 0 {
            return Err(TransferError::NonPositiveAmount);
        }
        if payer.balance < amount {
            return Err(TransferError::InsufficientFunds);
        }
        let credit = payee
            .balance
            .checked_add(amount)
            .ok_or(TransferError::DestinationOverflow)?;
        self.0.get_mut(source).unwrap().balance -= amount;
        self.0.get_mut(destination).unwrap().balance = credit;
        Ok(())
    }
}
