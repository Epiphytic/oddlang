# Component publication on macOS Apple Silicon

The release, WIT recovery and public CLI tests exercise the ledger, its instrumented world, and an independently authored echo contract. Every test runs with the workspace Cargo lock and the local Cargo cache.

```sh
cargo test --locked --offline --test wasm_release --test wasm_import_wit --test wasm_cli
scripts/verify-wasm-spike.sh
```

The native Rust guest produced a component for `wasm32-wasip2`. An independent Wasmtime host confirmed that its linker required zero definitions and observed exact echoes for five signed integers including both extrema. The [recorded observations](../../examples/wasm/evidence/observations.txt) contain the component and extracted WIT digests. Repeated builds on the qualification host produced the same component digest.

During implementation, independent fixtures caught an identity collision between differently typed parameters, partial overwrite of an existing recovery directory, and an unresolved type in the instrumented world. The regression cases are in [the recovery suite](../../tests/wasm_import_wit.rs) and [the publication suite](../../tests/wasm_release.rs). The [CLI suite](../../tests/wasm_cli.rs) exercises both host/guest directions and both report formats.

## Claude review

Claude Code reviewed commit `4eca8ea` in auto mode in a disposable checkout. It reran the 11 original integration tests and the offline component spike. Its verdict was **revise**. It requested the missing profile specification and a production pin for the instrumented world. It also identified the nominal WIT interface identity as part of the inclusion contract. The setup script accesses the network to populate the caches. The phase 2 runtime gate owns observed compatibility claims.

The revised candidate includes the [closed profile and diagnostic rules](../language/wasm-core.md). The instrumented world supplies only its additional declarations, imports the pinned production ontology, and references the production WIT interface by its versioned name. Tests cover drift in a direct include and preservation of the interface through a transitive include. A separate pinned-import test verifies WIT dependencies against a consumer-held lock.

Claude approved the corrected candidate at `5667d3f` after a clean-checkout replay of all 15 component tests and the offline spike. It independently rejected a missing ordinal, an inverted balance interval, a resource, an incorrect pin and altered cached WIT. The complete prior HTTP regression gate and its eight evidence reports also passed. The measured component build digest applies to the tested macOS Apple Silicon host.
