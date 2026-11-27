# Public unsigned constant recovery

- [x] Recover a selected public crate-root integer from a pinned source file
  and matching compiler rustdoc 57 export. Retain only literal and product
  semantics in the shared-declarations graph.
- [x] Verify an unrelated real rustdoc fixture and a source mutation, then
  compile downstream callers of the original and projected constants.
- [x] Qualify the separately composed prototype export on both native ARM64
  hosts and preserve the six sibling member locks and the private buffered
  reader in the [companion release](https://github.com/Epiphytic/marmish-oddlang/pull/87).

`oddlang recover public-constant SOURCE RUSTDOC SELECTION OUTPUT` publishes
`graph.json`, `rust.json` and `recovery.lock` atomically. The selector sets
`format` to `oddlang-public-constant-selection-v1` and supplies `id`,
`constantId`, `crateName`, `name`, `sourceFilename`, `sourceSha256`
and `rustdocSha256`. Generate the document with Rust 1.97.1 rustdoc JSON.
The document must describe the exact crate root and source coordinates,
public visibility, a canonical primitive type, and an evaluated numeric
value equal to the checked source expression. Recovery preserves grouped
multiplication as semantic value expressions. It supports unsigned
`u8`, `u16`, `u32`, `u64` and `usize`; the compiler witness and
downstream compile each apply their native target bounds for `usize`.
The receipt binds evidence bytes. Source and compiler artifacts are
recovery-time inputs, and the existing shared-declarations build consumes
only the portable graph and Rust overlay.

Run the generic acceptance fixture with
`cargo run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin public-constant -- path/to/oddlang`.
