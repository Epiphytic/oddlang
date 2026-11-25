#![allow(dead_code)]

mod original {
    include!("original.rs");

    pub fn fields(row: &Account) -> (&str, &str, i64) {
        (&row.id, &row.owner, row.balance)
    }
}

mod generated {
    include!("generated.rs");
}

use std::collections::BTreeMap;

fn named_error(error: original::TransferError) -> &'static str {
    use original::TransferError::*;
    match error {
        SourceMissing => "urn:oddlang:ledger:error:SourceMissing",
        DestinationMissing => "urn:oddlang:ledger:error:DestinationMissing",
        SameAccount => "urn:oddlang:ledger:error:SameAccount",
        NonPositiveAmount => "urn:oddlang:ledger:error:NonPositiveAmount",
        InsufficientFunds => "urn:oddlang:ledger:error:InsufficientFunds",
        DestinationOverflow => "urn:oddlang:ledger:error:DestinationOverflow",
    }
}

type Outcome = Result<(), &'static str>;
fn compare(rows: &[(&str, &str, i64)], source: &str, destination: &str, amount: i64) -> Outcome {
    let mut before = BTreeMap::new();
    let mut after = generated::__operations::State::default();
    for &(id, owner, balance) in rows {
        before.insert(
            id.to_owned(),
            original::Account::new(id.into(), owner.into(), balance).unwrap(),
        );
        after
            .insert(generated::Account::new(id.into(), owner.into(), i128::from(balance)).unwrap());
    }
    let expected =
        original::transfer(&mut before, source, destination, amount).map_err(named_error);
    let actual = generated::__operations::lookup("urn:oddlang:ledger:TransferFunds").unwrap()(
        &mut after,
        source,
        destination,
        amount,
    );
    assert!(actual == expected, "exact named result differs");
    assert!(before.len() == after.rows().len(), "key count differs");
    // Both input maps were built from these distinct keys. Checking their presence,
    // together with the final length, establishes equality of the complete key sets.
    assert!(before.len() == rows.len());
    for &(key, _, _) in rows {
        let left = original::fields(&before[key]);
        let right = &after.rows()[key];
        assert!(
            left == (right.id(), right.owner(), right.balance()),
            "complete row differs"
        );
    }
    actual
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(64)]
fn transfer_equivalence() {
    let same: bool = kani::any();
    let result = compare(&[], "s", if same { "s" } else { "d" }, kani::any());
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:SourceMissing"),
        "empty source missing"
    );
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(64)]
fn populated_equivalence() {
    let debit: i64 = kani::any();
    let credit: i64 = kani::any();
    let bystander: i64 = kani::any();
    kani::assume(debit >= 0 && credit >= 0 && bystander >= 0);
    let source = if kani::any() { "s" } else { "x" };
    let dest = match kani::any::<u8>() {
        0 => "s",
        1 => "x",
        _ => "d",
    };
    let result = compare(
        &[("s", "a", debit), ("d", "b", credit), ("z", "c", bystander)],
        source,
        dest,
        kani::any(),
    );
    kani::cover!(result.is_ok(), "success");
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:SourceMissing"),
        "source missing"
    );
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:DestinationMissing"),
        "destination missing"
    );
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:SameAccount"),
        "same account"
    );
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:NonPositiveAmount"),
        "nonpositive"
    );
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:InsufficientFunds"),
        "insufficient funds"
    );
    kani::cover!(
        result == Err("urn:oddlang:ledger:error:DestinationOverflow"),
        "overflow"
    );
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(64)]
fn guard_priority_counterexample() {
    let amount: i64 = kani::any();
    kani::assume(amount == -1);
    let _ = compare(&[], "s", "d", amount);
}
