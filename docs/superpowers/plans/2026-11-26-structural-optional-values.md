# Structural recovery of an optional unsigned range

- [x] Build a closed source matcher for an unsigned optional tuple value, its validating constructor, accessor, default, declared maximum and named unit error.
- [x] Derive the existing portable value-domain graph from that structure. Use the sealed native API observer independently and reject disagreement.
- [x] Observe pinned Marmish core and a source mutation. Generate Rust and compare the same callers and unchanged core tests before and after.

## Supported form

The package library exposes the selected value and error as direct public
re-exports from a conventional external module. The selection names source
declarations and assigns stable IDs. Width and the range-error identity are
recovered from the declaration and the constructor's failure path.

A closed Rust shape admits one optional unsigned tuple field, an accessor
that reads it, and a default literal or checked arithmetic expression.
The constructor rejects present values via a disjunction of literal option
matches or simple optional comparisons. The complement must be one closed
interval and must agree with the declared maximum. The named error table
comes from literal unit-variant diagnostics. Unsupported syntax, shadowed
prelude names, and active attributes on the selected behavior fail recovery.

Recovery uses a clean revision export and emits the existing target-neutral
value vocabulary. Before publication it verifies the structurally derived
graph against the separately compiled native caller.
