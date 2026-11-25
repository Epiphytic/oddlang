mod account;
use account::{__operations, Account};
use std::io::{self, BufRead};

fn main() {
    let ids = ["A", "a", "C", "missing", "also-missing"];
    let owners = ["Ada", "", "Cy"];
    let operation = __operations::lookup("urn:oddlang:ledger:TransferFunds").unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut fields = line.split_whitespace();
        let mut state = __operations::State::default();
        for i in 0..3 {
            let balance = fields.next().unwrap().parse().unwrap();
            state.insert(Account::new(ids[i].into(), owners[i].into(), balance).unwrap());
        }
        let source: usize = fields.next().unwrap().parse().unwrap();
        let destination: usize = fields.next().unwrap().parse().unwrap();
        let amount = fields.next().unwrap().parse().unwrap();
        let result: Result<(), &'static str> =
            operation(&mut state, ids[source], ids[destination], amount);
        print!("{}", result.err().unwrap_or("ok"));
        for (key, account) in state.rows() {
            print!(
                "\t{}\t{}\t{}\t{}",
                key,
                account.id(),
                account.owner(),
                account.balance()
            );
        }
        println!();
    }
}
