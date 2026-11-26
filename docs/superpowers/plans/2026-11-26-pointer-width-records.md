# Pointer-width public record fields

- [x] Recover a public Rust `usize` field as a target pointer-width unsigned value, and lower it to an explicitly qualified native primitive.
- [x] Verify a data-only fixture with exact native assignment, zero and maximum boundaries, a fixed-width source mutation, and a primitive-shadow rejection.
- [x] Preserve deterministic recovery and render locks and verify the existing public-record fixtures.

The portable field type is `pointer-unsigned`. It has the unsigned domain of the selected target's pointer width. The Rust projection uses `::core::primitive::usize`. Serialization follows the declared native Serialize capability. Consumers with a fixed wire-width requirement should select a fixed-width field type.

The module selection format `oddlang-rust-public-record-candidate-selection-v1` permits unresolved qualified globs that do not start with crate, self or super and, for relative paths, whose prefix is not locally bound, including by a pinned glob. Its recovery lock records their spellings and requires independent native confirmation of selected types, traits and public paths. The strict format still rejects those globs, and candidates retain pinned validation of recognized local glob sources. Generation consumes the portable graph and overlay.

Run `python3 verification/observational-record-defaults/pointer_width.py target/debug/oddlang`.
