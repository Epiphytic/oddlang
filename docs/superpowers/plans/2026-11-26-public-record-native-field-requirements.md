# Check Rust source API requirements on public record fields

**Goal.** Validate field requirements for a Rust drop-in source API during native projection and preserve existing overlays.

- [x] Prove a downstream caller can use the named public optional unsigned field.
- [x] Reject a mismatched field name, visibility, type, role, or identity before projection.
- [x] Recheck portable wire semantics, legacy output, and exact review.

The optional Rust overlay property `sourceApi` contains role `rust-drop-in-source-api` and a nonempty `fields` map keyed by stable field IRI. Each entry asserts the exact emitted name, public visibility, and structural native type. The type uses the existing checked type vocabulary. An optional unsigned 64-bit field uses `{"optional":"u64"}` to assert the Rust `Option<u64>` type.

The role binding adds an obligation for a source-compatible Rust caller. The public record contract's optional unsigned 64-bit JSON field independently accepts omission or null as open, emits null for open, and preserves integer values up to `18446744073709551615`. Consumers implementing the wire role can use their own native representation and test the portable wire cases.

The [macOS ARM64 receipt](../../../verification/public-records/receipts/source-api-aarch64-apple-darwin.json) records the unchanged native caller, eight wire cases, seven binding rejections, Serde source recovery, and byte equality for the 11 legacy artifacts. Replay the fixture with `python3 -B verification/public-records/source_api.py PATH_TO_ODDLANG`.

- [x] Qualify the MissingRange endpoint against the independent shared JSON cases. On macOS ARM64, the Rust and TypeScript consumers preserved the exact integer `9007199254740993`, including with replacement or absent hints. An unchanged downstream Rust caller constructed the record and read its public `end: Option<u64>` field. The new seven-package manifest selects that checked Rust binding. [Wire and native role evidence](https://github.com/Epiphytic/marmish-oddlang/blob/be1de6c244732424c3b6b0bc9502152b2e4eb425/verification/whole/gap-range-endpoint/README.md)
