# Recover Serde public records

> **For agentic workers:** Use superpowers:executing-plans to track these checks.

**Goal.** Recover the source-declared Rust record shape and Serde input policy for one selected public record, including an optional unsigned field and a selected public enum reference.

**Evidence.** Pair pinned Rust module bytes with pinned Rustdoc JSON format 57 from the same crate with private items included. Require matching source spans, public fields, type and trait identities, and a checked crate-root reexport. Selection supplies source names and stable semantic identities only.

- [x] Add an unrelated checked Rustdoc fixture, source-only policy mutation, and rejection controls.
- [x] Recover the graph and Rust overlay using the existing public-record renderer, preserving legacy recovery bytes.
- [x] Verify and record review, fixture results, and remaining scope.

Run `oddlang recover serde-public-record MODULE_RS RUSTDOC_JSON SELECTION_JSON OUTPUT`.
Generate format 57 evidence from the library with `cargo rustdoc --lib -- -Z unstable-options --output-format json --document-private-items` using Rust 1.97.1 and the toolchain's unstable Rustdoc options.

The selector uses format `oddlang-serde-public-record-selection-v1` and has these keys.

- `crateName`, `moduleName`, and `sourceFilename` select the source module in Rustdoc. The source filename is the relative spelling stored in the Rustdoc spans.
- `sourceSha256`, `crateRootSha256`, and `rustdocSha256` pin the module, its sibling `lib.rs`, and the Rustdoc bytes.
- `moduleId`, `record: {name, id, fields}`, and `choices: [{name, id}]` bind stable identities. The `fields` map keys are source field spellings. Rustdoc resolves the field type and the source declaration controls field order and read policy.

The selected record lives in a top-level sibling file module. The record and any linked enum require an unambiguous crate-root export, witnessed by the source and Rustdoc. Fields are public `bool`, `u32`, `u64`, `usize`, or a selected fieldless enum from that same source module. The standard `Option<T>` is supported once around any of these values. The stock Serde derives admit object and positional sequence input. Unknown object members are ignored by default and rejected when the source declares `#[serde(deny_unknown_fields)]`. The stock `Option` field accepts absent and null values. Other Serde attributes, derives outside the explicit profile, unresolved globs in the selected module, and source/compiler identity mismatches fail closed.

The output files are `graph.json`, `rust.json`, and `recovery.lock`. Qualification must use independently produced Rustdoc evidence from the pinned build and an independent downstream caller to confirm Serde behavior. The linked enum's semantic contract is qualified separately by its own recovery and generation tooling.

The [macOS ARM64 receipt](../../../verification/public-records/receipts/serde-recovery-aarch64-apple-darwin.json) records three native source and projection passes, eight rejection controls, and byte equality for the 11 predecessor artifacts. Replay the generic native check with `python3 -B verification/public-records/recover_serde.py PATH_TO_ODDLANG`.
