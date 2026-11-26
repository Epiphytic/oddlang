# Recover closed Serde string choices

- [x] Recover a digest-pinned public Rust unit enum with the supported stock and Serde derives. Map native names to caller-supplied stable identities and preserve source order and exact default or symmetric renamed string spellings.
- [x] Reject unsupported serialization transforms, ambiguous derive imports, payloads, explicit discriminants and additional selected definitions. Publish portable shared declarations, a separate Rust overlay and a recovery lock.
- [x] Compile unrelated original and projected native callers, exercise type and wire differences under a real source mutation, and check deterministic locks.

Run `oddlang recover serde-string-choice SOURCE SELECTION OUTPUT`. The `oddlang-rust-serde-string-choice-selection-v1` selection provides `id`, `enumId`, `name`, `sourceSha256`, and a complete `variants` map from Rust names to stable IRIs. The result contains the existing shared-declarations graph, Rust overlay, and a recovery lock. The graph stores exact wire strings and the copyability constraint. The overlay stores Rust names and declaration order. The lock pins all inputs and outputs by digest.

The source profile admits exactly Clone, Debug, PartialEq, Eq, Serde Serialize and Deserialize, with optional Copy. Serde derives may be qualified or explicitly imported from Serde, including aliases. The default variant spelling and symmetric `#[serde(rename = "...")]` are supported. This profile rejects enum-level options, directional renames, extra variant attributes, manual selected impls and glob imports. The compiler-independent result is a syntax candidate. Sibling macro expansion, actual derive resolution and external public paths remain native qualification obligations.

Run the [independent native fixture](../../../verification/serde-string-choice/verify.py) after building Oddlang.

```sh
python3 verification/serde-string-choice/verify.py target/debug/oddlang
```
