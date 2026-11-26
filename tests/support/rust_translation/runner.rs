// Independently compiled against the original or emitted module. Empty and arbitrary
// UTF-8 fields survive the line protocol via byte-wise hexadecimal encoding.
use std::io::{self, BufRead};

fn text(s: &str) -> String {
    let bytes = s
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    String::from_utf8(bytes).unwrap()
}
fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

#[cfg(original)]
mod implementation {
    include!(env!("TRANSLATION_SOURCE"));
    use std::collections::BTreeMap;
    pub type State = BTreeMap<String, Account>;
    pub fn insert(state: &mut State, id: String, owner: String, balance: i64) {
        state.insert(id.clone(), Account::new(id, owner, balance).unwrap());
    }
    pub fn run(state: &mut State, s: &str, d: &str, n: i64) -> &'static str {
        match transfer(state, s, d, n) {
            Ok(()) => "ok",
            Err(TransferError::SourceMissing) => "urn:oddlang:ledger:error:SourceMissing",
            Err(TransferError::DestinationMissing) => "urn:oddlang:ledger:error:DestinationMissing",
            Err(TransferError::SameAccount) => "urn:oddlang:ledger:error:SameAccount",
            Err(TransferError::NonPositiveAmount) => "urn:oddlang:ledger:error:NonPositiveAmount",
            Err(TransferError::InsufficientFunds) => "urn:oddlang:ledger:error:InsufficientFunds",
            Err(TransferError::DestinationOverflow) => {
                "urn:oddlang:ledger:error:DestinationOverflow"
            }
        }
    }
    pub fn visit(state: State, mut row: impl FnMut(&str, &str, &str, i64)) {
        for (key, a) in state {
            row(&key, &a.id, &a.owner, a.balance);
        }
    }
}

#[cfg(not(original))]
mod implementation {
    include!(env!("TRANSLATION_SOURCE"));
    pub use __operations::State;
    pub fn insert(state: &mut State, id: String, owner: String, balance: i64) {
        state.insert(Account::new(id, owner, balance.into()).unwrap());
    }
    pub fn run(state: &mut State, s: &str, d: &str, n: i64) -> &'static str {
        __operations::lookup("urn:oddlang:ledger:TransferFunds").unwrap()(state, s, d, n)
            .err()
            .unwrap_or("ok")
    }
    pub fn visit(state: State, mut row: impl FnMut(&str, &str, &str, i64)) {
        for (key, a) in state.rows() {
            row(key, a.id(), a.owner(), a.balance());
        }
    }
}

fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut fields = line.split('\t');
        let s = text(fields.next().unwrap());
        let d = text(fields.next().unwrap());
        let n = fields.next().unwrap().parse().unwrap();
        let mut state = implementation::State::default();
        while let Some(key) = fields.next() {
            let owner = text(fields.next().unwrap());
            let balance = fields.next().unwrap().parse().unwrap();
            implementation::insert(&mut state, text(key), owner, balance);
        }
        let status = implementation::run(&mut state, &s, &d, n);
        print!("{status}");
        implementation::visit(state, |key, id, owner, balance| {
            print!("\t{}\t{}\t{}\t{balance}", hex(key), hex(id), hex(owner));
        });
        println!();
    }
}
