# Rust ledger import profile v1

## Import and replay

From the repository root, run the following with the toolchain pinned by the project.

```sh
cargo run --locked --offline -- import rust examples/rust-import/ledger/Cargo.toml examples/rust-import/ledger/mapping.toml target/import-ledger
cargo run --locked --offline -- build target/import-ledger target/import-ledger-native
cargo run --locked --offline -- import replay examples/rust-import/ledger/Cargo.toml examples/rust-import/ledger/mapping.toml target/import-ledger
cargo run --locked --offline -- import verify-build target/import-ledger target/import-ledger-native
```

Use a fresh import directory. Its import.lock pins the inputs and published artifacts. Replay rebuilds the import and compares locks. The verify-build command regenerates the native projections and checks the delivered bytes. To edit the semantic model independently, copy the publication into a separate directory, remove import.lock from that copy, edit its Turtle and run the ordinary build command on the copy. The original publication remains available for replay.

The fixture supplies Cargo.toml, Cargo.lock and src/lib.rs. Its mapping.toml selects the ledger-v1 profile and names the source declarations. The Rust, SQL and Avro projections and the field declarations live under overlays/ beside the mapping. An accepted cleanup also records the original tree, effective tree and patch using the [cleanup skill](../../skills/oddlang-import-cleanup/SKILL.md).

## Syntax and effects

The profile selects one dependency-free Cargo library with a single source file. Its Rust items are a named account struct, its inherent checked constructor, a unit-variant error enum and a transfer function. The importer parses the entire file with Syn and requires a successful locked Cargo build. Module inclusions, attributes (including derives and cfg), macros, imports, other functions and trait implementations are outside the profile. The observation domain is a single-threaded call on a valid finite entry map with keys equal to account IDs and balances within the declared bounds. Allocation succeeds.

The mapping supplies the Rust declaration names and stable IRIs. Map the three account fields (two private String fields and a private i64 balance), the three value arguments, the operation, each error variant and its corresponding guard. The constructor takes owned fields in id, owner, balance order, returns `Option<Self>`, rejects a negative balance with an early None and returns `Some(Self { ... })` with each original field. This profile checks bounds 0 through i64::MAX. External projections supply storage and wire names.

The operation takes a mutable `std::collections::BTreeMap<String, Account>`, two borrowed str keys in source and destination order, and an i64 amount. It returns `Result<(), Error>`. A top-level if with no else and a single `return Err(Error::Variant)` introduces a guard. Its RDF condition denotes **success**. A failed contains_key test becomes Exists. Equality of the two keys becomes NotEqual. Failed numeric comparisons `<=`, `<` and `>` become Greater, GreaterEqual and LessEqual. Guard priority follows statement order. The error variant selects the stable guard IRI independently of order. Each mapped variant occurs exactly once.

An immutable local can bind a pure integer expression over arguments, earlier locals, literals and account balance snapshots. Integer widening uses i128::from. An indexed balance read requires an earlier successful existence guard on that key. Widened plus and minus use signed i128, with conservative entry-interval checking at each operator. The maximum-balance constant can be written i128::from(i64::MAX). Expansion has the ledger-core expression limits. Names resolve only to admitted bindings and standard types. Local shadowing is excluded.

The commit consists of one balance store per key via `state.get_mut(key).unwrap().balance`. Both keys must exist and a preceding guard must establish their distinctness. Each right-hand side is a previously computed widened local cast to i64. Successful numeric guards and entry bounds must imply the mapped balance bounds for both candidates. The importer checks affine inequalities by eliminating the three entry variables over the rationals, encoding strict integer bounds with an offset of one. An inconclusive or excessively complex proof rejects the store. After the first store, only further checked stores and a final Ok(()) are admitted. All snapshot reads and calculations therefore precede mutation. The editable graph buffers these assignments with the same observable result.

Diagnostics identify the source file and the one-based starting line and column of the rejected node. The source map also records exclusive end positions. Locations, local names and comments are outside the semantic graph.
