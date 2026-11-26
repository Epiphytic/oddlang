# Bounded Sequence Journal Implementation Plan

The [language contract](../../language/sequence-journal.md) owns the
primitive state and ordered-case semantics.

- [ ] Add an unrelated telemetry queue fixture and independent expected
  outputs for partial trim, bytes and record bounds, exact retry, FIFO receipt
  eviction, finalization and effect rollback. Verify the CLI fixture initially
  fails for the absent projection.
- [ ] Validate graph identities, typed expressions and limits, complete
  ordered cases and total native bindings. Preserve case order in the
  deterministic graph and overlay digests. Reject invalid types, unsafe
  names, effectful read operations and unmatched rule sets.
- [ ] Lower graph cases to native methods over a bounded byte/receipt
  store. Prepare each effect from the original state and publish only
  the checked staged transition. Share byte records across staging.
- [ ] Add the CLI and native assembly component. Compile the fixture in
  Rust 2021 and compare independently expected outcomes, guard-order controls,
  capacity mutations and deterministic replay. Update the work graph with
  evidence and run independent review before publication.
