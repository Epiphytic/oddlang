# Terrain block conformance

Run on Rust 1.97.1 using the existing offline Cargo cache.

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 -B verification/integrity-block/verify.py target/debug/oddlang
```

The unrelated Rust 2021 consumer checks ordered JSON, tuple acceptance, canonical field encodings, checked end positions, typed failures and integrity. Static zlib fixtures were prepared with the Python standard library independently of the emitted Rust. The consumer rejects trailing bytes, multiple members, preset dictionaries, truncation, short and excess decoded output, incorrect digests and unnegotiated compression. It verifies identity-only generation, absence of provider-hint effects on source and obligations, deterministic generation, and the effects of changing field labels and validation precedence. Malformed graph controls fail before publication.
