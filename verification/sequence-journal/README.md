# Sequence journal qualification

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 -B verification/sequence-journal/verify.py target/debug/oddlang
```

The unrelated telemetry queue exercises a five-byte journal, two retained
records and two independent receipts. The Rust 2021 caller covers partial
acknowledgement, byte credit and full-slot credit, same-content retries,
conflicting retries, a receipt evicted after its delivery, gap errors,
bounded reads, idempotent finish and exact retry after finalization.
A second state starts one position below the unsigned cursor limit and
tests rejection at the first overflowing append.

Two successful-to-failing effect sequences check that a late effect error
discards the staged append and restores an evicted receipt. A 256-case
sweep checks byte preservation, retry recognition and conflicts across
three payload lengths. A priority mutant changes a cursor-overflow error
to a gap. A record-capacity mutant changes the independently expected
credit.

The verifier compares graph-equivalent Rust files, locks and obligations
byte for byte. It rejects invalid operation typing, incomplete method
bindings, named expression cycles and missing references, invalid phase
rules and altered digests. It also assembles the same queue into a native
library with a separately projected error enum; the caller uses its
public module and explicit root reexport.

The private storage fixture also compiles the telemetry types with explicitly
named cursors, byte records and receipts. Its lexical child tests initialize
the high and low cursors in a struct literal and inspect the actual queue
as it is appended and evicted. A downstream field-access probe is rejected.
The same public caller passes in the aggregate and named-field modes.

A separate native macOS ARM64 test assembled and built the production stream
window package in an isolated scratch workspace. After the release build,
it appended the original test module verbatim to the generated journal
leaf with surrounding test-scope imports. All five tests passed on original
and projected crates. The original `windows.rs` test module has SHA-256
`34cdb8cbbdc542171ae9602d857520bd82c35cf9fb921f2143664fa82e73cff7`.
The test module was a qualification input. [Pinned upstream test source](https://github.com/Epiphytic/marmish/blob/af54339d250290490d62d4475a77539b7e739071/crates/marmish-stream/src/windows.rs)

Claude Code with Claude Opus 5.5 approved the implementation in auto permission
mode and reran the journal, shared declarations and native-package
qualifications. Its Rust name collision finding led to an early rejection
for projected type names that shadow generated runtime names. The verifier
covers that rejection.
