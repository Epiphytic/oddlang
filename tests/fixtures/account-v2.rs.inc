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
    currency_code: String,
}

impl Account {
    pub fn new(id: String, owner: String, balance: i128, currency_code: String) -> Result<Self, &'static str> {
        Ok(Self {
            id,
            owner,
            balance: Balance::try_from(balance)?,
            currency_code,
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

    pub fn currency_code(&self) -> &str {
        &self.currency_code
    }
}
