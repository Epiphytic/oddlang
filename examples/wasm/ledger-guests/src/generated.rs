use super::{Account, TransferError};

#[allow(unused_parens)]
mod model {
    include!("account.rs");
}

pub struct State(model::__operations::State);

fn to_wire(account: &model::Account) -> Account {
    Account {
        id: account.id().to_owned(),
        owner: account.owner().to_owned(),
        balance: account.balance(),
    }
}

impl State {
    pub fn from_accounts(accounts: Vec<Account>) -> Self {
        let mut state = model::__operations::State::default();
        for row in accounts {
            let account = model::Account::new(row.id, row.owner, i128::from(row.balance))
                .expect("initial balance outside the ledger contract");
            state.insert(account);
        }
        Self(state)
    }

    pub fn lookup(&self, id: &str) -> Option<Account> {
        self.0.rows().get(id).map(to_wire)
    }

    #[cfg(feature = "instrumented")]
    pub fn snapshot(&self) -> Vec<Account> {
        self.0.rows().values().map(to_wire).collect()
    }

    pub fn transfer(
        &mut self,
        source: &str,
        destination: &str,
        amount: i64,
    ) -> Result<(), TransferError> {
        let execute = model::__operations::lookup("urn:oddlang:ledger:TransferFunds").unwrap();
        execute(&mut self.0, source, destination, amount).map_err(|iri| match iri {
            "urn:oddlang:ledger:error:SourceMissing" => TransferError::SourceMissing,
            "urn:oddlang:ledger:error:DestinationMissing" => TransferError::DestinationMissing,
            "urn:oddlang:ledger:error:SameAccount" => TransferError::SameAccount,
            "urn:oddlang:ledger:error:NonPositiveAmount" => TransferError::NonPositiveAmount,
            "urn:oddlang:ledger:error:InsufficientFunds" => TransferError::InsufficientFunds,
            "urn:oddlang:ledger:error:DestinationOverflow" => TransferError::DestinationOverflow,
            _ => panic!("unmapped ledger error"),
        })
    }
}
