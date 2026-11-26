# Endpoint-set qualification

Build Oddlang and run the unrelated download-mirror consumer.

```sh
cargo +1.97.1 build --locked --offline --bin oddlang
python3 -B verification/endpoint-set/verify.py target/debug/oddlang
```

The run projects two equivalent inputs and compares both generated files
byte for byte. It compiles the bound type, free operation and an altered
policy with a separately locked URL-provider dependency. The consumer checks
first errors, default-port handling, DNS syntax and label boundaries, terminal
dots, IPv4 and IPv6 prefixes, loopback opt-in, ordered deduplication, and
re-normalization. It tests all 256 addresses in a sample IPv4 subnet against an
independently written range expectation. The altered policy allows empty input,
preserves duplicates and terminal dots, and changes the first error when two
raw-character rules match.

Controls reject an incorrect digest, an input rule after parsing, nonzero
CIDR host bits, an undeclared error, incomplete enum bindings and a malformed
Rust identifier.
