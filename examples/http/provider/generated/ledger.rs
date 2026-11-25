#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Balance(i64);

impl TryFrom<i128> for Balance {
    type Error = &'static str;
    fn try_from(value: i128) -> Result<Self, Self::Error> {
        if (0_i128..=9223372036854775807_i128).contains(&value) {
            Ok(Self(value as i64))
        } else {
            Err("urn:oddlang:ledger:balance")
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    id: String,
    owner: String,
    balance: Balance,
}

impl Account {
    pub fn new(id: String, owner: String, balance: i128) -> Result<Self, &'static str> {
        Ok(Self {
            id,
            owner,
            balance: Balance::try_from(balance)?,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn balance(&self) -> i64 {
        self.balance.0
    }
}

pub mod __operations {
    #[derive(Default)]
    pub struct State(std::collections::BTreeMap<String, super::Account>);
    impl State {
        /// Upsert a checked record using its own identity as the map key.
        pub fn insert(&mut self, account: super::Account) {
            self.0.insert(account.id().to_owned(), account);
        }
        pub fn rows(&self) -> &std::collections::BTreeMap<String, super::Account> { &self.0 }
    }
    pub type Operation = fn(&mut State, &str, &str, i64) -> Result<(), &'static str>;
    pub fn lookup(iri: &str) -> Option<Operation> {
        match iri {
            "urn:oddlang:ledger:TransferFunds" => Some(operation_0),
            _ => None,
        }
    }
    fn operation_0(state: &mut State, source: &str, destination: &str, amount: i64) -> Result<(), &'static str> {
        if !(state.0.contains_key(source)) { return Err("urn:oddlang:ledger:error:SourceMissing"); }
        if !(state.0.contains_key(destination)) { return Err("urn:oddlang:ledger:error:DestinationMissing"); }
        if !((source != destination)) { return Err("urn:oddlang:ledger:error:SameAccount"); }
        if !((i128::from(amount) > 0_i128)) { return Err("urn:oddlang:ledger:error:NonPositiveAmount"); }
        if !((i128::from(state.0[source].balance()) >= i128::from(amount))) { return Err("urn:oddlang:ledger:error:InsufficientFunds"); }
        if !(((i128::from(state.0[destination].balance()) + i128::from(amount)) <= 9223372036854775807_i128)) { return Err("urn:oddlang:ledger:error:DestinationOverflow"); }
        if source == destination { return Err("urn:oddlang:ledger:error:AssignmentConflict"); }
        let value_0 = super::Balance::try_from((i128::from(state.0[destination].balance()) + i128::from(amount))).map_err(|_| "urn:oddlang:ledger:error:FieldOutOfRange")?;
        let value_1 = super::Balance::try_from((i128::from(state.0[source].balance()) - i128::from(amount))).map_err(|_| "urn:oddlang:ledger:error:FieldOutOfRange")?;
        state.0.get_mut(destination).expect("guarded target").balance = value_0;
        state.0.get_mut(source).expect("guarded target").balance = value_1;
        Ok(())
    }
}
