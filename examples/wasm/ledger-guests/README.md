# Ledger component guests

The handwritten guest implements transfer directly. The generated guest runs the Rust operation emitted by `oddlang build examples/ledger/v1`. Both use the released WIT for bindings.

## Supplying state

Supply one row per unique, case-sensitive account ID. Each row carries its own ID, a string owner, and a balance between 0 and 9223372036854775807 inclusive. An empty snapshot is valid. These preconditions apply to both the host's `initial-state` response and the test world's `fixture-load` argument. [Ledger entry-state semantics](../../../docs/language/ledger-core.md#execution)

On the first export call, the guest calls `initial-state` once and retains its state for that instance. This also applies when the first call is `fixture-load`, which then replaces the entire state. The test world's `complete-snapshot` returns all rows in ID order.

Hosts should validate incoming snapshots before supplying them. In direct Wasmtime calls with out-of-contract inputs, both guest implementations retained the last row for a repeated ID and trapped on a negative balance. The same outcomes occurred through the import and through fixture replacement. These observations describe the current artifacts.

## Building and checking the components

After running `cargo run --locked --offline --manifest-path verification/wasm-setup/Cargo.toml`, run the following from the repository root with published `ledger/` and `ledger-test/` bundles and a fresh output directory.

```sh
cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml \
  --bin wasm-guests -- BUNDLES OUT
```

The offline build uses the pinned Rust freestanding target, wraps each core module with `wasm-tools component new`, validates the final component, and checks its type against the published world. It also compares the final component's extracted WIT with the full import and export surface derived from the release. The production artifacts export `lookup` and `transfer`. Only the test artifacts export `fixture-load` and `complete-snapshot`. Both worlds import the ledger types and the host's `initial-state` function.
