# Finite nested error categories

Recover ordinary finite enums of errors from pinned Rust modules. Each
variant has a stable identity, a unit or one-element payload, an explicit
display rule, and independent source and conversion markers. Category tables
map symbolic external enum members to exact strings. The Rust overlay binds
type and item names, variant order and external enum paths. The renderer emits
an exhaustive native match so the Rust compiler checks the table against the
actual dependency. The package assembler exposes the result alias and error
type at the group and crate root.

The recovery entry point recognizes a bounded `thiserror` pattern using
`syn`, a SHA-256-pinned module and an identity-only selector. It produces a
candidate graph, Rust overlay and observation record. The contract scope
covers literal and one-insertion diagnostics, text payloads, I/O source and
conversion, and statically bound enum categories. Compiler and downstream
runtime qualification establish the supported observations.

- [x] Add finite error recovery and Rust projection with a separate native
  binding and hashed obligations.
- [x] Compile an unrelated consumer across a category mutation and check
  rejection of unsupported Rust, malformed graphs and non-exhaustive native
  dependencies.
- [x] Compile one unchanged downstream Marmish caller against pinned original
  and generated v6-based workspaces, exhaustively exercising 10 error
  variants and the 24 nested copy and stream categories.
- [x] Qualify a source category mutation and verify public I/O redaction,
  retained native source and the generic result aliases in both lanes.
