# Canonical JSON wire regression

Build the compiler with Rust 1.97.1, then run the unrelated native radio fixture.

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 -B verification/canonical-wire/verify.py target/debug/oddlang
```

The fixture compiles the generated Rust as an edition 2021 module with warnings denied. A handwritten caller checks exact directional bytes, malformed canonical envelopes, separate size and admission errors, validation precedence, timed admission, ordered optional and tagged scopes, external result propagation, byte aggregate limits, hexadecimal fields and independently expected SHA-256 bytes. The build driver checks determinism, verifies emitted hashes, and rejects incomplete mappings, a misplaced clock scope, conflicting declaration IDs and private structural API leaks.
