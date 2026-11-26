# Ordered JSON wire schemas

The `oddlang-json-wire-schema-v1` graph describes named closed records and adjacent tagged unions. Stable IRIs identify definitions, fields, cases and validation guards. The `oddlang-json-wire-schema-rust-v1` binding supplies native names, derives and external Rust paths. Text enum contracts, compression blocks and other separately qualified wire types can appear as named external references.

```sh
oddlang recover wire-schema SOURCE_RS SELECTION_JSON OBSERVED_DIR
oddlang build wire-schema rust GRAPH_JSON RUST_BINDING_JSON OUTPUT_DIR
```

The recover command checks the source bytes against `sourceSha256` in the selection. It reads selected top-level Rust declarations and recovers ordered fields, named types and vetted Serde attributes. The profile understands explicit `rename`, enum `rename_all = "snake_case"`, `tag`, `content`, `deny_unknown_fields`, and declared `with` adapter identities for fixed lowercase hexadecimal bytes and bounded Base64 byte lists. The selection supplies adapter policy, not an explanation inferred from the helper body. Conditional declarations, unsupported attributes and unresolved type references become explicit structural gaps. Structural candidates also list validation behavior as unobserved. The observation pins source, selection, graph and native binding by SHA-256.

The build command consumes only the graph and binding. It rejects unresolved shapes or source evidence gaps, invalid type references, recursion, binding collisions and ill-typed guard operands. It writes `wire.rs`, `obligations.json` and a lock covering both outputs and their inputs. Externally supplied types remain separate qualification dependencies. The generator relies on the native Serde provider to implement the declared shape.

## Shape and guard semantics

- Records emit an object with fields in declared order. Serde also accepts positional tuples for named structs. An optional value accepts a missing key and JSON null, and the native projection emits its key and null when that value is absent.
- Adjacent unions declare separate tag and content keys. Each case has an exact wire name and declares a unit, one named value, or an ordered record payload. Unit cases emit the tag without content. Closed shapes reject unknown and duplicate object fields.
- Field values can be booleans, text, unsigned 8-, 16-, 32- or 64-bit integers, optional values, ordered lists, named references, or a declared byte adapter on a named field. The Base64-list adapter receives item, encoded-byte, aggregate-byte and per-item accounting rules. It validates list size, encoded text length, decoding and canonical padding, and total decoded bytes in order. Overall frame bounds belong to the caller before parsing.
- A validation guard selects a record or a named record case, optionally checks a `when` predicate, and requires a typed predicate or returns its stable error IRI. Guards run in declaration order. The initial profile supports unsigned ranges, boolean and presence checks, list counts and uniqueness, and unsigned field comparisons. A guard applies only to its declared owner and case. Validation of a referenced value is a separate operation.
- The emitted validator is independent of Serde admission. Byte-canonical envelope validation, input-to-envelope mappings, typed foreign validation calls and time-sensitive checks require additional declarations and independent evidence.

The locked Rust 2021 fixture compares original and generated wire bytes, rejects malformed records and codecs, and changes a simultaneous-failure result by moving one guard. It also detects a changed source pin, an unverified Serde attribute and an ill-typed guard. [Verification](../../verification/wire-schema/verify.py)
