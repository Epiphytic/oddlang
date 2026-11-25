# Rust ledger translation validation

The production build uses the independently pinned source fixture and the graph emitted by the public Rust importer. The translation gate recompiles the generated code, compares its outputs and full state with the original implementation and the independent oracle, and runs a bounded verification experiment on both production function bodies. Each proof result identifies its input domain and assumptions.

```sh
verification/rust-translation/scripts/setup.sh
scripts/verify-rust-translation.sh
```

The verification directory contains the original pin, import lock, generated native code, differential results, verifier output and a reproducible summary. Evidence from the Wasm guests applies to the specific component releases in the [component execution record](component-execution-evidence.md).
## Native observations

Both compiled implementations agreed with the independent oracle on the first named result and every row in 1,922 cases. These included 1,024 seeded cases, 125 successes and witnesses for all six specified errors. The cleaned fixture also agreed with the frozen pre-edit original. The early-debit mutant returned the expected overflow error but reduced the source balance. The guard-order mutant returned NonPositiveAmount on an empty map with amount −1, where the contract requires SourceMissing.

The ledger input source digest is `186160a9fdb96b07b10aabe34e6c84d662df169801d07477d6be1ae303323b5f`. The regenerated Rust digest is `96ca54999c57b7577073fc5abe360a86417f058d3ce3de98f91b9b6234c6d21a`. The corpus and proof bind those exact bytes independently.

## Proof scope

Kani 0.68.0 with CBMC 6.11.0 discharged the empty-map comparison with an arbitrary signed 64-bit transfer amount, including equal and distinct query keys. The harness reached the specified SourceMissing result. This result uses Kani's pinned nightly compiler, successful allocations and the checked production function bodies. The independent work-graph task for arbitrary maps remains blocked on key-renaming and bystander-extension proofs.

The separate populated harness used three fixed keys, an unrelated bystander, symbolic nonnegative balances, and every signed amount. CBMC reached its 90-second limit. Equivalence of that domain is inconclusive. The negative control on the empty map failed exactly the result comparison, and the solver-generated concrete test reproduced the failure.

Claude CLI in auto mode approved the scoped claims at `0f56c51` after independently rerunning the gate. Claude rejected a fabricated populated proof and detected changes to the native build and playback transcript. Four further mutations changed observed behavior. A fifth, special-casing the literal key “x”, survived the corpus. The empty-map proof exercises only SourceMissing. The early-debit witness exercises the overflow return. Downstream work should cite individual classifications in the archived summary.

[Download the complete replay archive](../../examples/rust-import/evidence/translation.tar.gz). Extract it to an empty directory, then run `python3 verification/translation-evidence/verify.py "$PWD" EXTRACTED_DIRECTORY` in the reviewed checkout.
