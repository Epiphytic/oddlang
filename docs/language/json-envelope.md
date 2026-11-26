# Bounded JSON envelope profile

The profile describes one record with an exact unsigned literal and a binary field encoded as canonical padded standard Base64. The graph supplies wire field names, their order, byte limits, named errors and the order of observable guards. A Rust binding supplies the public API names.

```sh
oddlang build json-envelope rust GRAPH_DIR RUST_BINDING_JSON OUTPUT_DIR
```

The projection writes `codec.rs` and `projection.lock`. The lock records SHA-256 digests of the canonical envelope model, Rust binding and generated file. The generated Rust uses Serde, serde_json and Base64.

- A named `JsonEnvelope` links two `JsonEnvelopeField` declarations with `field`. Give each field a distinct nonempty `wireName` and consecutive `position` beginning at zero. Set `fieldKind` to `UnsignedLiteral` for the field with exact unsigned `value` and `bits` of 8, 16, 32 or 64. Set the other field to `CanonicalBase64`.
- The `shape` is either `ClosedRequiredObject` or `ClosedRequiredObjectOrTuple`. Both require both fields, reject duplicate and unknown object keys, and emit compact JSON objects in declaration order. The latter shape also accepts a positional array of exactly two elements in declaration order. Decoding accepts JSON whitespace and any order of object keys.
- The envelope declares `rawBytes` for decoded data, `wireBytes` for JSON input and output, and `recordLineBytes` as an exported limit for a caller's enclosing line-based protocol. The codec operates on a single JSON envelope.
- The encode track declares `encodeOrder "raw-size json wire-size"`. Decode declares `decodeOrder "wire-size json literal encoded-size base64 raw-size canonical"`, with alternative orders allowed if wire and JSON come first, the encoded size is bounded before Base64 allocation, and decoding precedes raw-size and canonical checks.
- The four roles `sizeError`, `jsonError`, `literalError` and `encodingError` link `NamedError` declarations with a stable IRI and a string `message`. The Rust binding maps each IRI to a public error variant.

The independent thumbnail consumer compiles against the same Rust API for multiple generated graphs. Its cases include exact object bytes, byte boundaries, guard precedence, malformed shapes and Base64, a changed field order and name, and object-only and tuple-capable readers. [Fixture graph](../../tests/fixtures/json-envelope/graph/domain.ttl) · [Rust binding](../../tests/fixtures/json-envelope/binding.json) · [Locked Cargo test](../../tests/json_envelope.rs)
