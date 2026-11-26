# Bounded Sequence Journal Implementation Plan

The [language contract](../../language/sequence-journal.md) owns the
primitive state and ordered-case semantics.

- [x] Add an unrelated telemetry queue fixture and independent expected
  outputs for partial trim, bytes and record bounds, exact retry, FIFO receipt
  eviction, finalization and effect rollback. Verify the CLI fixture initially
  fails for the absent projection.
- [x] Validate graph identities, typed expressions and limits, complete
  ordered cases and total native bindings. Preserve case order in the
  deterministic graph and overlay digests. Reject invalid types, unsafe
  names, effectful read operations and unmatched rule sets.
- [x] Lower graph cases to native methods over a bounded byte/receipt
  store. Prepare each effect from the original state and publish only
  the checked staged transition. Share byte records across staging.
- [x] Add the CLI and native assembly component. Compile the fixture in
  Rust 2021 and compare independently expected outcomes, guard-order controls,
  capacity mutations and deterministic replay. Update the work graph with
  evidence and run independent review before publication.

## Native private storage follow-up

- [x] Add an optional storage-name overlay for cursor, final marker, record
  and receipt fields. Emit the mapped native fields with private visibility.
  Evaluate expressions using a borrowed view of those fields and commit
  staged effects back to them only after the full transition succeeds.
- [x] Run an unrelated lexical child-module test that initializes private
  cursor fields and reads its actual receipt queue. Confirm that a downstream
  caller cannot access the fields directly.
- [x] Qualify both upstream private observations with the unchanged unit-test
  module embedded beside the authoritative generated fields. Keep the
  original test bytes in the verification stage after source-free generation.
