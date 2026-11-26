# Checked value domains

The checked-values graph defines finite scalar tables and checked products. Tables map each stable case to a unique text value or byte, with an optional exact parser and a named rejection. Text parsers match the input byte for byte. Values need valid UTF-8 at the API boundary. The Rust bindings choose the public enum, variant order, native view and parser names. Copy enums expose by-value views. Other enums use shared-reference views.

Products have a typed field catalog and an optional ordered constructor program. Each program checks borrowed text, primitive bytes and typed values before constructing an owned value. A failing guard returns the program's named error. An unsigned field can require membership in a finite set. Text guards can constrain UTF-8 byte count, require an ASCII byte alphabet, or reject explicit Unicode scalar intervals and General Category Cc. The Cc intervals are U+0000–001F and U+007F–009F. Interval bounds are inclusive. The overlay defines tuple or named representation, fields, getters, declaration order and derived Copy or Hash capability. Checked fields stay private and expose read-only getters. Records with public structural construction expose all members.

The shared declarations profile provides the named error type. Its optional category table supplies a unit variant's stable category separately from its Display segments. Rust maps the record's error IRI to the declared local error constructor. Generated method names preserve common trait call sites. The existing nullable unsigned profile continues to define optional range values, and the shared string-enum profile defines Serde wire spellings.

Run `oddlang build checked-values rust GRAPH OVERLAY OUTPUT`. The Rust projection emits a deterministic source artifact, portable declaration-level obligations, and a lock. A native package can assemble checked values with shared declarations. The unrelated verifier in `verification/checked-values/verify.py` compiles the assembled crate, exercises a UTF-8 length mutation, and checks canonical output and guard rejection.

The native package assembler resolves every checked error binding against a unit error variant emitted by a shared declaration or finite error domain in the same package. It checks the stable variant identity and the crate export path together. The projection lock format `oddlang-checked-values-lock-v1` uses `domainSha256` and `bindingSha256` for canonical declarations, while the package manifest pins the supplied file bytes.

An opaque `fixed-bytes` type declares an unsigned 32-bit `length`, a `failure` IRI for length mismatch, and `onDrop: "erase"`. Its native binding supplies a by-value constructor, a checked sequence constructor and a read-only array view through the `constructor`, `parse` and `view` keys. Supply empty `order` and omit fields and traits. The owner stores its own bytes, has a private backing field and exposes a borrow tied to its lifetime. The Rust backend uses the locked registry provider zeroize 1.9.1. Its destruction guarantee covers the object's current storage on drop. There is no requirement to clear a caller's separate input or copies left behind by moves.

Run `oddlang recover fixed-bytes OBSERVATION SELECTION OUTPUT` on a v2 rustc shape observation. Select the source declaration identity, domain IRI, output value IRI and failure IRI in the JSON keys `source`, `domain`, `value` and `failure`. Recovery reads the monomorphic tuple member, provider type, element and extent. The failure mapping and the public construction and borrowing interface require independent caller qualification. The resulting graph contains no source syntax or provider path. Run `python3 -B verification/checked-values/fixed_bytes.py /path/to/oddlang` to exercise an independent source-width change.

### Clone-only choices and derived Debug

A Rust recovery selection with format `oddlang-plain-choice-selection-v2`
accepts a public, fieldless enum that derives `Clone`, optionally
`Copy` and `Debug`, and its declared equality capabilities. Cases
retain declaration order and use implicit discriminants. Serde derives,
custom formatting and explicit discriminants require different profiles.

The choice graph defaults `copy` to true and `debug` to false. These
defaults preserve the earlier copyable profile's bytes. A clone-only choice
has `copy: false`, and the overlay omits the copy trait. A choice with
`debug: true` emits Rust's derived variant-name formatting. The original
v1 recovery selector retains its copyable, no-Debug scope.

Recover from a standalone declaration whose whole-file digest is pinned.
A qualification may extract an exact declaration span and stage it as that
file. It must independently establish the enclosing library's public paths,
trait resolution and missing capabilities.

The separate TypeScript plain-choice target promises copying and ordinal
conversion. A combined compiler must reject a graph that requires
clone-only semantics or derived Debug unless that target explicitly
implements the capability. Run the projection control below after
combining the targets. It first checks an admitted plain choice, then
requires both unsupported graph inputs to fail without an output.

```sh
python3 verification/plain-value-choices/typescript_subset.py target/release/oddlang
```
