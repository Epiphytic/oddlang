# Ledger component execution

The ledger guest uses freestanding Rust and Component Model bindings. While the small integer echo had zero imports on `wasm32-wasip2`, the ledger linked additional WASI imports on that target. The freestanding build exposes the published world, including the production namespace for account and transfer errors. Each guest build checks the extracted binary against that world.

```sh
scripts/setup-wasm.sh
scripts/verify-wasm.sh
```

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

The [qualification observations](../../examples/wasm/evidence/runtime-observations.json) record eight JavaScript host receipts, digests from both builds of all eleven components and the mutant traces. All eleven component digests were reproducible on this host. The trapping guest executed an unreachable instruction on the overlapping guard case. The double-call guest requested the snapshot twice in each production scenario.
