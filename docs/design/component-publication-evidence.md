# Component publication on macOS Apple Silicon

The release, WIT recovery and public CLI tests exercise the ledger, its instrumented world, and an independently authored echo contract. Every test runs with the workspace Cargo lock and the local Cargo cache.

```sh
cargo test --locked --offline --test wasm_release --test wasm_import_wit --test wasm_cli
scripts/verify-wasm-spike.sh
```

The native Rust guest produced a component for `wasm32-wasip2`. An independent Wasmtime host confirmed that its linker required zero definitions and observed exact echoes for five signed integers including both extrema. The [recorded observations](../../examples/wasm/evidence/observations.txt) contain the component and extracted WIT digests. Repeated builds on the qualification host produced the same component digest.

During implementation, independent fixtures caught an identity collision between differently typed parameters, partial overwrite of an existing recovery directory, and an unresolved type in the instrumented world. The regression cases are in [the recovery suite](../../tests/wasm_import_wit.rs) and [the publication suite](../../tests/wasm_release.rs). The [CLI suite](../../tests/wasm_cli.rs) exercises both host/guest directions and both report formats.
