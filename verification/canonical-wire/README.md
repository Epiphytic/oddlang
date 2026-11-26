# Canonical JSON wire regression

Build the compiler with Rust 1.97.1, then run the unrelated native radio fixture.

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 -B verification/canonical-wire/verify.py target/debug/oddlang
```

The fixture compiles the generated Rust as an edition 2021 module with warnings denied. A handwritten caller checks exact directional bytes, malformed canonical envelopes, separate size and admission errors, validation precedence, timed admission, ordered optional and tagged scopes, external result propagation, byte aggregate limits, hexadecimal fields and independently expected SHA-256 bytes. The build driver checks determinism, verifies emitted hashes, and rejects incomplete mappings, a misplaced clock scope, conflicting declaration IDs and private structural API leaks. A compile-and-run control covers singleton envelopes. A control without operation guards verifies that shape-only byte bounds still reject oversize collections before serialization or hashing.


The independent weather fixture exercises the standalone native package path with a closed canonical schema and generated shared error. It compiles the packaged library and an external caller with Rust 2021 warnings denied. It rejects missing shape pins, shape pins on unrelated components and a digest mismatch in either structural input. It also covers scalar byte limits and a frame whose JSON escaping exceeds the writer bound.

```sh
python3 -B verification/canonical-wire/package.py target/debug/oddlang
```
