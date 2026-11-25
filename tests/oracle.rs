#[path = "support/ledger_oracle.rs"]
mod ledger_oracle;

use ledger_oracle::{Account, State, transfer};

fn state(rows: &[(&str, &str, i64)]) -> State {
    rows.iter()
        .map(|&(id, owner, balance)| {
            (
                id.to_owned(),
                Account {
                    id: id.to_owned(),
                    owner: owner.to_owned(),
                    balance,
                },
            )
        })
        .collect()
}

#[test]
fn transfers_four_and_preserves_bystander_and_owners() {
    let mut actual = state(&[("a", "Ada", 10), ("A", "Bo", 3), ("c", "Cy", 7)]);
    assert_eq!(transfer(&mut actual, "a", "A", 4), Ok(()));
    assert_eq!(
        actual,
        state(&[("a", "Ada", 6), ("A", "Bo", 7), ("c", "Cy", 7)])
    );
}

#[test]
fn exact_headroom_and_full_source_balance_are_allowed() {
    let mut actual = state(&[("a", "Ada", i64::MAX), ("b", "Bo", 0)]);
    assert_eq!(transfer(&mut actual, "a", "b", i64::MAX), Ok(()));
    assert_eq!(actual, state(&[("a", "Ada", 0), ("b", "Bo", i64::MAX)]));
}

#[test]
fn first_failed_guard_returns_its_iri_and_preserves_every_field() {
    for (source, destination, amount, error) in [
        ("x", "x", 0, "urn:oddlang:ledger:error:SourceMissing"),
        ("a", "x", 0, "urn:oddlang:ledger:error:DestinationMissing"),
        ("a", "a", 0, "urn:oddlang:ledger:error:SameAccount"),
        (
            "a",
            "b",
            i64::MIN,
            "urn:oddlang:ledger:error:NonPositiveAmount",
        ),
        ("a", "b", 0, "urn:oddlang:ledger:error:NonPositiveAmount"),
        ("a", "b", 11, "urn:oddlang:ledger:error:InsufficientFunds"),
        ("a", "b", 1, "urn:oddlang:ledger:error:DestinationOverflow"),
    ] {
        let mut actual = state(&[("a", "Ada", 10), ("b", "Bo", i64::MAX), ("c", "Cy", 7)]);
        assert_eq!(
            transfer(&mut actual, source, destination, amount),
            Err(error)
        );
        assert_eq!(
            actual,
            state(&[("a", "Ada", 10), ("b", "Bo", i64::MAX), ("c", "Cy", 7)])
        );
    }
}
