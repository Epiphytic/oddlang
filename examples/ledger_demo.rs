#[path = "../tests/support/db.rs"]
mod db;
#[path = "../tests/support/events.rs"]
mod events;

use oddlang::{
    impact::impact,
    interpreter::{FieldValue, Row, State},
    model::{BALANCE, ID, OWNER, load_model},
    targets,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let writer = load_model("examples/ledger/v1")?;
    let reader = load_model("examples/ledger/v2")?;
    let output = tempfile::tempdir()?;
    for (version, model) in [("v1", &writer), ("v2", &reader)] {
        targets::build(model, &output.path().join(version))?;
        println!("{version} checked and built account.rs, schema.sql and account.avsc");
    }

    println!("Graph experiment");
    for iri in impact(&writer, BALANCE)? {
        println!("  {iri} depends on balance");
    }
    println!(
        "The comparison in docs/design/impact-comparison.md records eight edges in both fixtures and the SPARQL and TOML traversal costs."
    );

    println!("Target conformance");
    let mut state = State::new(&writer);
    for (id, owner, n) in [("a", "Ada", 30), ("b", "Bea", 5)] {
        state.insert(Row::from([
            (ID.into(), FieldValue::Text(id.into())),
            (OWNER.into(), FieldValue::Text(owner.into())),
            (BALANCE.into(), FieldValue::Integer(n)),
        ]))?;
    }
    let transfer = "urn:oddlang:ledger:TransferFunds";
    state.execute(transfer, "a", "b", 7)?;
    assert_eq!(state.rows()["a"][BALANCE], FieldValue::Integer(23));
    assert_eq!(state.rows()["b"][BALANCE], FieldValue::Integer(12));
    println!("Transfer of 7 leaves balances 23 and 12");
    let before = state.rows().clone();
    let error = state
        .execute(transfer, "missing", "missing", -1)
        .unwrap_err();
    assert_eq!(error, "urn:oddlang:ledger:error:SourceMissing");
    assert_eq!(&before, state.rows());
    println!("Two missing accounts and a negative amount produce {error}");
    let mut db = db::TestDatabase::connect()?;
    let result = events::evolution_witness(&mut db.client, &writer, &reader);
    println!(
        "Saved v1 event and migrated PostgreSQL row agree on {:?}",
        result.historical
    );
    println!(
        "New v2 event and PostgreSQL row agree on {:?}",
        result.current
    );
    println!(
        "Removing the reader default yields {}",
        result.missing_default
    );
    Ok(())
}
