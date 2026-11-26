# Converge public API recovery in one seven-member workspace

- [x] Compile and qualify the public constant and typed error recovery commands in the same offline compiler. Run both unrelated fixtures and the library tests. Validate the design graph.
- [x] Compose the qualified engine and native package inputs with the prototype constant package. Verify each member's manifest and source digest against its independently qualified parent. Preserve the existing workspace Cargo lock.
- [x] Rebuild the workspace on both native ARM64 hosts with original Rust inputs absent. Test public observations against both original and generated libraries, including exact constant value, typed errors, native choice and preserved reader behavior.
- [x] Rerun the public import audit against the combined Darwin build. Publish the input hashes, provenance, per-import outcomes and the remaining gaps. Obtain an exact Claude auto-mode review for each phase and merge the companion evidence.

The integration compiler combines public constant recovery at `c70bc9b` and typed error recovery at `bbdc69e`. The public constant and typed finite error fixtures passed with its compiled CLI. All 13 library tests passed with `cargo +1.96.1 test --target-dir /tmp/odd-buffer-compiler --locked --offline --lib` and no inherited `CARGO_TARGET_DIR`. The round-trip importer fixture inspects Cargo’s target location, so exporting that variable for the test process changes its temporary package behavior.

The [combined workspace qualification](https://github.com/Epiphytic/marmish-oddlang/pull/89) merged at `4f39fe3da13635d66b5a09fbe55e52fe27d17962`. The cold builds on Linux and Darwin ARM64 agree on the generated workspace lock and public engine, identity and constant observations. The Darwin caller audit increased importability from 65 to 70 of the 140 original names. The named suite and mixed-peer interoperability require separate evidence.

## Named tests for the recovered release parser

- [x] Identify upstream private parser tests by their original harness and exact names. Stage the pinned test module and included version vectors against the generated private parser. Measure both native release catalogs.

The source-free workspace passed 42 of 909 active original tests on Darwin ARM64 and 43 of 887 on Linux ARM64, with five additional ignored cases on each. The new cases are `native_manifest::tests::strict_vectors` and `native_manifest::tests::shared_release_version_vectors`. The independent 224-case parser corpus contributes zero to these counts. Both full-suite certification checks exited with status 1. [Catalog receipts and reproduction instructions](https://github.com/Epiphytic/marmish-oddlang/tree/cb6e38f463fece606f6b0e8c5fb2f920c46dfa16/verification/whole/suite)
