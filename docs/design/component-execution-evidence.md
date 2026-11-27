# Ledger component execution

The ledger guest uses freestanding Rust and Component Model bindings. While the small integer echo had zero imports on `wasm32-wasip2`, the ledger linked additional WASI imports on that target. The freestanding build exposes the published world, including the production namespace for account and transfer errors. Each guest build checks the extracted binary against that world.

```sh
cargo run --locked --offline --manifest-path verification/wasm-setup/Cargo.toml
cargo +1.97.1 run --locked --offline --manifest-path verification/provider-validation/Cargo.toml --bin wasm-verify
```

Use `--prepare EMPTY_DIRECTORY` after Cargo’s `--` to produce one set of artifacts for the Wasmtime integration test. The full command builds two sets and runs all three hosts. To replay with previously built binaries, pass `--built-tools DIR` containing `oddlang`, `publish-wasm-matrix`, `wasm-guests`, `runtime-host`, the `wasm_runtime` test executable, `wasm-js-hosts`, and `wasm-reports`. Both modes check the wasm-tools archive against the repository pin.

The verification directory contains both clean builds, their digest comparison, the WIT and canonical ABI for every guest, and separate receipts for each host and participant. The ledger operations run in four binaries across the handwritten and generated implementations and the production and instrumented worlds. Node and Chromium instantiate each transpiled guest through jco with the ledger snapshot import explicitly provided. They preserve signed 64-bit integers as BigInt at the ABI and as decimal strings in reports. Each observed case compares the full ordered import transcript, return values and named transfer errors with the published case.

The independently authored probe guest exercises the host's snapshot binding. The trapping guest and the guest that calls the snapshot import twice are negative controls. Their observations must fail evidence verification. The directional compatibility matrix records results for specific host and guest releases and the cases exercised.

On Apple Silicon the initial production runs returned these outcomes.

| Changed contract | Old host, new guest | New host, old guest |
| --- | --- | --- |
| Private label | Observed compatible | Observed compatible |
| Export rename | Type failure | Type failure |
| Swapped variant tags | Type failure | Type failure |
| Additional required host import | Link failure | Behavior failure |

In the final row the new guest requires a host function absent from the old host. In the reverse direction the old guest loads, but its observed import transcript omits the call expected by the new contract.

The trapping mutant reached a Wasm `unreachable` on the transfer in `ordered-guards-and-wide-transfer`. The duplicate-import mutant called `initial-state` twice in all three production cases. Both failed evidence verification. A handwritten and a generated production component passed those same cases. The independent ledger oracle also checked the expected state and error for every published ledger step.

The [qualification observations](../../examples/wasm/evidence/runtime-observations.json) record eight JavaScript host receipts, digests from both builds of all twelve components and the mutant traces. All twelve component digests were reproducible on this host. The trapping guest executed an unreachable instruction on the overlapping guard case. The double-call guest requested the snapshot twice in each production scenario.

## Claude review

Claude's auto-mode review of `9f37255` reran the offline component gate and all 15 publication tests. It introduced a mutant that rejected a credit taking a destination balance to exactly the maximum signed 64-bit value. The initial corpus accepted that mutant. The reviewer requested an explicit successful transfer at the upper endpoint. The existing HTTP regression suite ran independently with PostgreSQL 16.6 and passed its 85 tests and eight evidence checks.

The revised corpus seeds a destination at `i64::MAX - 1` and credits one unit. Both reference guests succeed and subsequent lookups verify the balances. Replaying Claude's original mutant against that release produced `destination-overflow` on the first call of `exact-destination-headroom`. The qualification script also builds a permanent version of the mutant and verifies the counterexample.

Claude approved `ee20f9f` after repeating the entire component gate in a fresh checkout. It reproduced the checked-in observation report and verified the rejected upper-bound mutant. Its disposable checkout lacked the HTTP npm dependencies. Separately, the initialized HTTP gate passed all 101 applicable tests and its eight evidence checks on the same implementation.
