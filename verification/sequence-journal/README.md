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
