mod binding {
    include!("../generated/binding.rs");
}
mod ledger {
    include!("../generated/ledger.rs");
}

use binding::{Decimal, Lookup, Pair, Service, Transfer};
use ledger::__operations::{State, lookup};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

struct Ledger(Mutex<State>);
fn account(state: &State, id: &str) -> Result<binding::Account, &'static str> {
    let a = state
        .rows()
        .get(id)
        .ok_or("urn:oddlang:http:error:AccountMissing")?;
    Ok(binding::Account {
        id: a.id().into(),
        owner: a.owner().into(),
        balance: Decimal::new(a.balance())?,
    })
}
impl Service for Ledger {
    fn lookup(&self, request: Lookup) -> Result<binding::Account, &'static str> {
        account(&self.0.lock().unwrap(), &request.id)
    }
    fn transfer(&self, request: Transfer) -> Result<Pair, &'static str> {
        let mut state = self.0.lock().unwrap();
        let op = lookup(binding::BEHAVIOR).ok_or("unavailable ledger operation")?;
        op(
            &mut state,
            &request.source,
            &request.destination,
            request.amount.value(),
        )?;
        Ok(Pair {
            source: account(&state, &request.source)?,
            destination: account(&state, &request.destination)?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    id: String,
    owner: String,
    balance: Decimal<0, { i64::MAX }>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [fixture_flag, file, listen_flag, addr] = args.as_slice() else {
        return Err("expected --fixture FILE --listen HOST:PORT".into());
    };
    if fixture_flag != "--fixture" || listen_flag != "--listen" {
        return Err("expected --fixture FILE --listen HOST:PORT".into());
    }
    let fixture: Vec<Fixture> = serde_json::from_slice(&std::fs::read(file)?)?;
    let mut state = State::default();
    for a in fixture {
        state.insert(ledger::Account::new(
            a.id,
            a.owner,
            i128::from(a.balance.value()),
        )?);
    }
    let app = binding::router(Arc::new(Ledger(Mutex::new(state))));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("listening {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
