# Rust source recovery

The independent ledger crate pins its manifest, Cargo lock and original source in [original.sha256](../../examples/rust-import/ledger/original.sha256). The import validates Rust 1.96.1, compiles with overflow checks, lowers the selected operation to Turtle and emits the source map separately.

The CLI tests import the crate, invoke the existing native emitter, and run a downstream Rust executable with the original source unavailable. Swapping the first two guards changes the named error for two missing accounts. Replacing the credit operand changes the observed destination balance. Source formatting and a local rename preserve every Turtle file. Invalid casts, identity collisions, post-commit reads, unsupported effects and stale provenance reject the translation.

```sh
cargo test --locked --offline --test rust_import --test rust_cleanup --test rust_replay
```

On macOS Apple Silicon, the initial fixture's semantic graph had SHA-256 `ded5662c8609dd33fdf439b5003cf008e4b7fd700f9cf72dcb474425144969c1`. Input and output file hashes are stored separately in the emitted import lock.
