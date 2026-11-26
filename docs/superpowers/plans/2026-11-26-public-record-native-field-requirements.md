# Check Rust source API requirements on public record fields

**Goal.** Validate field requirements for a Rust drop-in source API during native projection and preserve existing overlays.

- [ ] Prove a downstream caller can use the named public optional unsigned field.
- [ ] Reject a mismatched field name, visibility, type, role, or identity before projection.
- [ ] Recheck portable wire semantics, legacy output, and exact review.

The optional Rust overlay property `sourceApi` contains role `rust-drop-in-source-api` and a nonempty `fields` map keyed by stable field IRI. Each entry asserts the exact emitted name, public visibility, and structural native type. The type uses the existing checked type vocabulary. An optional unsigned 64-bit field uses `{"optional":"u64"}` to assert the Rust `Option<u64>` type.

The role binding adds an obligation for a source-compatible Rust caller. The public record contract's optional unsigned 64-bit JSON field independently accepts omission or null as open, emits null for open, and preserves integer values up to `18446744073709551615`. Consumers implementing the wire role can use their own native representation and test the portable wire cases.
